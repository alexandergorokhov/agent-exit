# Agent Exit. Every target either does real work or prints "not built yet"
# and exits non-zero. Nothing here fakes success.
#
# Offline targets: verify, test, lint, audit (needs the advisory DB), hash, demo.
# Targets whose help line says "(not built yet)" print that and exit non-zero.
# Testnet targets are marked [testnet]; they never touch mainnet.

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

PROP_CASES ?= 10000
N ?= 200
SEED ?= 42
CLEAN ?= 50
FIXTURE ?=
OUT := out

define not_built
	@echo "not built yet: $(1) (planned: $(2))" >&2; exit 1
endef

.PHONY: help
help: ## List targets
	@grep -E '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/' | sort

# ---------------------------------------------------------------- offline core

.PHONY: setup
setup: ## Print toolchain versions; fetch crates and npm packages (network once)
	rustc --version
	cargo --version
	cargo fetch --locked
	@command -v node >/dev/null && { node --version; cd mcp && npm ci --no-audit --no-fund; } || echo "node not installed: the MCP server (mcp/) needs Node >= 20" >&2

.PHONY: test
test: ## Unit + property tests (offline)
	PROP_CASES=$(PROP_CASES) cargo test --workspace --locked --offline

.PHONY: verify
verify: ## Offline rerun: tests + fixed world -> out/verify.json, compared with tests/golden/verify.json
	@mkdir -p $(OUT)
	PROP_CASES=$(PROP_CASES) cargo test --workspace --locked --offline --quiet
	cargo run --quiet --locked --offline -p verify -- --json --out $(OUT)/verify.json --golden tests/golden/verify.json
	cargo run --quiet --locked --offline -p attack -- --out $(OUT)/attack.json --golden tests/golden/attack.json > /dev/null
	cargo run --quiet --locked --offline -p verify --bin business-loop -- --out $(OUT)/business-loop.json --golden tests/golden/business-loop.json

.PHONY: business-loop
business-loop: ## 30 days of the agent business with no human after the setup: customers pay, investor paid 10%, runner buys summarizer jobs inside the cap -> out/business-loop.json
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p verify --bin business-loop -- --out $(OUT)/business-loop.json --golden tests/golden/business-loop.json

.PHONY: golden
golden: ## Rewrite tests/golden/verify.json from the current code (review the diff!)
	cargo run --quiet --locked --offline -p verify -- --json --out tests/golden/verify.json
	cargo run --quiet --locked --offline -p attack -- --out tests/golden/attack.json > /dev/null
	cargo run --quiet --locked --offline -p verify --bin business-loop -- --out tests/golden/business-loop.json

.PHONY: mcp-golden
mcp-golden: mcp-build ## Rewrite fixtures/mcp/world.json, tests/golden/agent-buys-agent.json and web/data/demo.json (review the diff!)
	cargo run --quiet --locked --offline -p verify --bin gen-mcp-world
	node mcp/dist/scripts/agent-buys-agent.js --out tests/golden/agent-buys-agent.json
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo

# ---------------------------------------------------------------- demo page (web/)

WEB_PORT ?= 8080
NODE_GLOBAL := $(shell npm root -g 2>/dev/null)

.PHONY: web-data web-testnet-data
web-testnet-data: ## Rewrite web/data/testnet.json from the recorded testnet run in out/scenes
	node scripts/testnet-page-data.mjs
	node scripts/diligence-data.mjs
	node scripts/judge-mcp-config.mjs

web-data: ## Rewrite web/data/demo.json from the contract (review the diff!)
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo

.PHONY: web
web: ## Serve the demo page on http://127.0.0.1:8080 (offline, recorded; ?network=testnet adds read-only live reads)
	@command -v node >/dev/null || { echo "node not installed: the page server needs Node >= 20" >&2; exit 1; }
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo -- --check
	node web/serve.mjs $(WEB_PORT)

.PHONY: check-links
check-links: ## Every tx the page links is known to NEAR testnet (archival RPC; needs network)
	node scripts/check-tx-links.mjs

.PHONY: attest attest-check
attest: ## [online, $$0] G7: fetch NEAR AI Cloud's attestation report for the G6 model (no completion, no charge) with a fresh nonce; check it binds the contract's signer (TDX quote via Phala's DCAP verifier, GPU via NVIDIA NRAS) -> out/gates/g7-attestation.json
	node scripts/attest/verify-nearai-attestation.mjs fetch

attest-check: ## [offline] G7: re-check the saved attestation report (network checks show as unverified) and run its tests
	node scripts/attest/verify-nearai-attestation.mjs check --offline
	node --test scripts/attest/test/verify.test.mjs

.PHONY: web-test
web-test: ## Playwright smoke test of the page (Chromium, desktop + 375px, no network)
	@command -v node >/dev/null || { echo "node not installed: web-test needs Node >= 20" >&2; exit 1; }
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo -- --check
	node --test web/test/relay.test.mjs
	NODE_PATH="$${NODE_PATH:+$$NODE_PATH:}$(NODE_GLOBAL)" node web/test/smoke.mjs

# ---------------------------------------------------------------- MCP + Skill

.PHONY: mcp-build
mcp-build: ## Build the MCP server (TypeScript) and the scan binary it calls
	@command -v node >/dev/null || { echo "node not installed: the MCP server needs Node >= 20" >&2; exit 1; }
	cargo build --quiet --locked --offline -p scan
	cd mcp && { test -d node_modules || npm ci --no-audit --no-fund; } && npx tsc -p .

.PHONY: mcp
mcp: mcp-build ## Start the MCP server on stdio (offline by default; AGENT_EXIT_NETWORK=testnet; never mainnet)
	@node mcp/dist/src/server.js

.PHONY: judge judge-testnet screen mcp-build-if-stale
# judge skips the build (no cargo, no tsc) when mcp/dist and the scan binary are
# newer than the sources; otherwise it runs mcp-build.
# The scan binary: AGENT_EXIT_SCAN_BIN if set, else the first that exists of
# target/{debug,release}/scan and $CARGO_TARGET_DIR/{debug,release}/scan.
# Exported to the judge run (the MCP client passes it on to the server).
SCAN_FIND = for p in "$${AGENT_EXIT_SCAN_BIN:-}" $(CURDIR)/target/debug/scan $(CURDIR)/target/release/scan"$${CARGO_TARGET_DIR:-target}/debug/scan" "$${CARGO_TARGET_DIR:-target}/release/scan"; do if [ -n "$$p" ] && [ -x "$$p" ]; then echo "$$p"; break; fi; done
mcp-build-if-stale:
	@if [ -f mcp/dist/scripts/judge-check.js ] && [ -n "$$($(SCAN_FIND))" ] \
	  && [ -z "$$(find mcp/src mcp/scripts schemas -newer mcp/dist/scripts/judge-check.js -print -quit)" ]; then :; else $(MAKE) -s mcp-build; fi

# Exit 4 = setup error (scan binary not found; no claim judged).
judge: mcp-build-if-stale ## For a judge's agent: start the MCP server from .mcp.json, call check_claims, K0 code hash + one PASS/FAIL row per README claim (K1-K8, offline; 9 rows; about 1 s once built) -> out/check-claims.json
	@scan="$$($(SCAN_FIND))"; if [ -n "$$scan" ]; then export AGENT_EXIT_SCAN_BIN="$$scan"; fi; node mcp/dist/scripts/judge-check.js

# The script exits 3 when no RPC answered; make then prints "Error 3" and exits 2.
# For the raw code run: node mcp/dist/scripts/judge-check.js --testnet
judge-testnet: mcp-build-if-stale ## [testnet, read-only, no key] K1-K9 from the agent-exit-testnet entry: recorded transactions re-read from archival nodes + live agents -> out/check-claims-testnet.json. No RPC: NOT CHECKED rows, make fails with Error 3 (exit 2); node mcp/dist/scripts/judge-check.js --testnet alone exits 3
	@scan="$$($(SCAN_FIND))"; if [ -n "$$scan" ]; then export AGENT_EXIT_SCAN_BIN="$$scan"; fi; node mcp/dist/scripts/judge-check.js --testnet

screen: ## The README's main offline claims, 7 rows with their promised outcome (1-3 min; make verify, attack and fuzz run separately) -> out/screen.json
	@bash scripts/screen.sh

.PHONY: mcp-test
mcp-test: mcp-build ## MCP tests offline (S14, S15, E29, E30, schemas, skill) + agent-buys-agent vs its golden
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p verify --bin gen-mcp-world -- --check
	cd mcp && node --test --test-concurrency=4 'dist/test/*.test.js'
	node mcp/dist/scripts/agent-buys-agent.js --out $(OUT)/agent-buys-agent.json --golden tests/golden/agent-buys-agent.json
	$(MAKE) -s scripts-test
	cd packages/near-key-check && node --test 'test/*.test.mjs'

.PHONY: scripts-test
scripts-test: ## Offline: safety rules of scripts/testnet (testnet only, no secret echoed, DRY_RUN)
	node --test 'scripts/testnet/test/*.test.mjs'

.PHONY: agent-buys-agent
agent-buys-agent: mcp-build ## Scripted offline run: summarizer buys translator via MCP -> out/agent-buys-agent.json
	@mkdir -p $(OUT)
	node mcp/dist/scripts/agent-buys-agent.js --out $(OUT)/agent-buys-agent.json

.PHONY: llm-buyer
llm-buyer: mcp-build ## A real model (NEAR AI Cloud, else Anthropic) reads the Skill and buys via MCP, offline fixtures -> out/llm-buyer.json (needs NEARAI_API_KEY in ~/.agent-exit.env; exit 2 without)
	@mkdir -p $(OUT)
	node mcp/dist/scripts/llm-buyer.js --out $(OUT)/llm-buyer.json

.PHONY: translator-serve
translator-serve: mcp-build ## Translator worker on 127.0.0.1:8787: translates via NEAR AI Cloud and returns NEAR AI's signature to pay with (offline: recorded fixture, labelled; real: AGENT_EXIT_NETWORK=testnet + NEARAI_API_KEY + AGENT_EXIT_TRANSLATOR_ACCOUNT) [PORT=8787 MAX_JOBS=20]
	node mcp/dist/scripts/translator-serve.js --port $(or $(PORT),8787) --max-jobs $(or $(MAX_JOBS),20)

.PHONY: playground-wasm
playground-wasm: ## Build the page's playground: the real agent-core state machine + buyer scan as wasm -> web/playground.wasm
	cargo build --quiet --locked --offline --release --target wasm32-unknown-unknown -p playground
	cp "$${CARGO_TARGET_DIR:-target}"/wasm32-unknown-unknown/release/playground.wasm web/playground.wasm
	cd web && sha256sum playground.wasm > playground.wasm.sha256

.PHONY: agent-invests agent-invests-testnet
agent-invests: mcp-build ## Scripted investing agent via MCP (no model, $0): reads open offers, picks by counted revenue per 1%, buys, reads its portfolio; offline -> out/agent-invests.json
	@mkdir -p $(OUT)
	node mcp/dist/scripts/agent-invests.js --out $(OUT)/agent-invests.json

agent-invests-testnet: mcp-build ## [testnet] After make scene-12: the same agent, through carol's MCP server, buys the smallest part (2.5%) of the open offer on testnet (xUSD-FAKE) -> out/agent-invests-testnet.json
	node mcp/dist/scripts/agent-invests.js --network testnet --root $(TESTNET_ROOT) --out $(OUT)/agent-invests-testnet.json

.PHONY: llm-buyer-testnet
llm-buyer-testnet: mcp-build ## [testnet] After make scene-7: a real model, through carol's MCP server, scans both agents and buys on testnet (xUSD-FAKE) -> out/llm-buyer-testnet.json. ONE capped Anthropic run (~$0.05)
	node mcp/dist/scripts/llm-buyer.js --network testnet --root $(TESTNET_ROOT) --provider anthropic --max-steps 12 --max-tokens 150000 --out $(OUT)/llm-buyer-testnet.json

.PHONY: llm-buyer-check
llm-buyer-check: mcp-build ## One tiny authenticated chat call: is the NEARAI_API_KEY accepted and does the account have credit (exit 2 on 401/402 or no key)
	node mcp/dist/scripts/llm-buyer.js --check

.PHONY: attack
attack: ## Seeded backdoor corpus + clean controls + contract refusals -> out/attack.json [N=200 SEED=42 CLEAN=50 FIXTURE=fixtures/judge-attack.json]
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p attack -- --n $(N) --seed $(SEED) --clean $(CLEAN) $(if $(FIXTURE),--fixture $(FIXTURE)) --out $(OUT)/attack.json > /dev/null

.PHONY: check-account
key-check: ## Standalone, read-only, any testnet account: make key-check ID=name.testnet (packages/near-key-check; exit 0 locked, 1 not, 2 error)
	@node packages/near-key-check/cli.mjs '$(subst ','',$(ID))'

check-account: mcp-build ## Who else can take any NEAR account: make check-account ID=trading-bot.testnet [NETWORK=testnet] (exit 0 locked, 1 not locked, 2 error)
	@AGENT_EXIT_NETWORK='$(or $(NETWORK),offline)' node mcp/dist/scripts/check-account.js '$(subst ','',$(ID))' 

.PHONY: scan
scan: ## Scan one agent: make scan FILE=fixtures/scan/cheat.json (exit 0 clean, 2 blocked, 3 inconclusive)
	@s=0; cargo run --quiet --locked --offline -p scan -- $(FILE) || s=$$?; \
	case $$s in 0) echo "scan: CLEAN (exit 0)" >&2;; \
	  2) echo "scan: BLOCKED (exit 2): the agent is not safe to buy. For fixtures/scan/cheat.json this is the expected result; make then prints 'Error 2'." >&2;; \
	  3) echo "scan: INCONCLUSIVE (exit 3): do not buy" >&2;; esac; exit $$s

FUZZ_CASES ?= 3000
.PHONY: fuzz
fuzz: ## Property-based fuzzing of outsider inputs: ft_on_transfer msg, NEAR AI proofs, set_limits args, MPC answers [FUZZ_CASES=3000]
	FUZZ_CASES=$(FUZZ_CASES) cargo test --locked --offline -p agent-exit fuzz_ -- --test-threads=4

.PHONY: lint
lint: ## rustfmt check + clippy -D warnings + the ABI build (abi-check)
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	$(MAKE) --no-print-directory abi-check

# The ABI build compiles host code with near-sdk's ABI feature, which neither
# clippy nor the unit tests do; it broke unseen twice. Runs whenever cargo-near
# is installed and fails if either contract does not build.
.PHONY: abi-check
abi-check: ## cargo near build (agent + xusd, with ABI) when cargo-near is installed
	@if command -v cargo-near >/dev/null; then \
	  set -e; \
	  (cd contracts/agent && cargo near build non-reproducible-wasm --out-dir ../../target/near-abi-check) && \
	  (cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-abi-check) && \
	  echo "abi-check: agent and xusd build with ABI"; \
	else \
	  echo "abi-check: skipped, cargo-near not installed (cargo install cargo-near --locked)" >&2; \
	fi

.PHONY: audit
audit: ## cargo audit (needs cargo-audit + advisory DB) and check ignored advisories stay out of the wasm
	@command -v cargo-audit >/dev/null || { echo "cargo-audit not installed: cargo install cargo-audit --locked" >&2; exit 1; }
	cargo audit
	./scripts/ci/wasm-deps-clean.sh

.PHONY: ci
ci: lint verify mcp-test ## What CI runs offline (audit runs as its own CI job)

.PHONY: build
# The agent wasm compiles in the testnet token, admin, factory and MPC accounts (the
# same values as build-testnet), so the published hash is the deployed one.
# Needs Docker and a clean, committed tree (cargo-near builds a fresh clone).
build: ## Reproducible wasm build with cargo-near (Docker), testnet accounts compiled in -> contracts/res + SHA256SUMS
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.$(TESTNET_ROOT) AGENT_EXIT_ADMIN=$(TESTNET_ROOT) AGENT_EXIT_MPC=v1.signer-prod.testnet AGENT_EXIT_FACTORY=factory.$(TESTNET_ROOT) cargo near build reproducible-wasm --out-dir ../res
	cd contracts/xusd && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/factory && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/res && sha256sum *.wasm > SHA256SUMS

.PHONY: build-dev
build-dev: ## Non-reproducible wasm build with cargo-near, ABI included (no Docker)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && cargo near build non-reproducible-wasm --out-dir ../../target/near
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near
	cd contracts/factory && cargo near build non-reproducible-wasm --out-dir ../../target/near

.PHONY: hash
hash: ## Compare committed wasm with contracts/res/SHA256SUMS
	@test -f contracts/res/SHA256SUMS || { echo "not built yet: contracts/res/SHA256SUMS (run make build on a machine with Docker)" >&2; exit 1; }
	cd contracts/res && sha256sum -c SHA256SUMS

# ------------------------------------------------------------- real-node tests

.PHONY: test-sandbox test-sealed-sandbox test-born-sandbox
# neard raises its open-file limit to 65535 and exits if it cannot. Where the
# hard limit is lower (some containers), a tiny LD_PRELOAD shim clamps that
# one request to the hard limit.
FDSHIM = if ulimit -n 65535 2>/dev/null; then pre=""; else \
	  echo "sandbox: open-file hard limit is $$(ulimit -Hn), neard asks for 65535; loading fdshim" >&2; \
	  mkdir -p target && cc -shared -fPIC -O2 -o target/fdshim.so fdshim/fdshim.c -ldl; \
	  pre="$$PWD/target/fdshim.so"; fi;

test-sandbox: ## near-sandbox integration tests (S5, E5, A17, A21, SC-S1/S2 sealed dispute on a real local node)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.sandbox AGENT_EXIT_ADMIN=xusd.sandbox AGENT_EXIT_FACTORY=factory.sandbox cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd tests/sandbox && $(FDSHIM) LD_PRELOAD="$$pre" AGENT_WASM=../../target/near-sandbox/agent_exit.wasm XUSD_WASM=../../target/near-sandbox/xusd.wasm cargo test --locked -- --test-threads=1 --nocapture

test-born-sandbox: ## BC-S1: the born-clean factory creates an agent with no key on a real local node; a stranger cannot run new (~30 s)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.sandbox AGENT_EXIT_ADMIN=xusd.sandbox AGENT_EXIT_FACTORY=factory.sandbox cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd contracts/factory && cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd tests/sandbox && $(FDSHIM) LD_PRELOAD="$$pre" AGENT_WASM=../../target/near-sandbox/agent_exit.wasm FACTORY_WASM=../../target/near-sandbox/agent_factory.wasm cargo test --locked --test born -- --test-threads=1 --nocapture

test-sealed-sandbox: ## SC-S1/S2: sealed-config dispute + honest accept on a real local node, gas printed (~35 s)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.sandbox AGENT_EXIT_ADMIN=xusd.sandbox AGENT_EXIT_FACTORY=factory.sandbox cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd tests/sandbox && $(FDSHIM) LD_PRELOAD="$$pre" AGENT_WASM=../../target/near-sandbox/agent_exit.wasm XUSD_WASM=../../target/near-sandbox/xusd.wasm cargo test --locked --test sealed -- --test-threads=1 --nocapture

# Stress scenarios (docs/SCENARIOS.md "Stress (STR-*)"): scale and hostile load.
# Offline part always runs; the MCP part needs Node; the sandbox part needs
# cargo-near and the near-sandbox binary (and an fd limit of 65535 for neard).
# STR_BUGS=1 also runs the scenarios marked BUG (they fail until fixed).
.PHONY: stress stress-offline stress-mcp stress-sandbox
stress: stress-offline stress-mcp stress-sandbox ## Stress scenarios STR-*: offline + MCP + sandbox [STR_BUGS=1 runs the BUG-marked ones too]

stress-offline: ## STR-01..15 in the mocked runtime (no network, ~4 min)
	cargo test -p verify --features stress --test stress --locked --offline -- --nocapture --test-threads=1 $(if $(STR_BUGS),--include-ignored)

stress-mcp: mcp-build ## STR-16/17 daily spend cap under parallel buy_agent calls
	cd mcp && STR_RUN_BUGS=$(STR_BUGS) AGENT_EXIT_SCAN_BIN=$(abspath $(or $(CARGO_TARGET_DIR),target))/debug/scan node --test dist/test/stress.stress.js

stress-sandbox: ## STR-S1..S5 on a real local node (needs cargo-near; skipped with a message when missing)
	@if ! command -v cargo-near >/dev/null; then echo "stress-sandbox: skipped, cargo-near not installed" >&2; exit 0; fi
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.sandbox AGENT_EXIT_ADMIN=xusd.sandbox AGENT_EXIT_FACTORY=factory.sandbox cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd tests/sandbox && $(FDSHIM) LD_PRELOAD="$$pre" AGENT_WASM=../../target/near-sandbox/agent_exit.wasm XUSD_WASM=../../target/near-sandbox/xusd.wasm cargo test --locked --test stress -- --test-threads=1 --nocapture

# ------------------------------------------------------------- testnet day
# Runs on the owner's machine (NEAR RPC, Base Sepolia and NEAR AI Cloud are
# reachable there). Reads ~/.agent-exit.env (see .env.example); refuses
# anything but testnet. DRY_RUN=1 prints the plan offline and sends nothing.

TESTNET_ROOT ?= $(shell sed -n 's/^AGENT_EXIT_ROOT=//p' $(HOME)/.agent-exit.env 2>/dev/null | tail -1)
TESTNET_ROOT := $(if $(TESTNET_ROOT),$(TESTNET_ROOT),agentexit.testnet)
TESTNET_RUN = cd tools/testnet && cargo run --quiet --release --locked --
PLAN = cargo run --quiet --locked --offline -p testnet-plan --
# Indirect on purpose: GNU make runs a recipe line that names $(MAKE) even
# under `make -n`, which once sent a real deploy. Use DRY_RUN=1 to preview.
SUBMAKE = $(MAKE)

.PHONY: build-testnet testnet-setup deploy-testnet seed-testnet gates gate-g1 gate-g2 gate-g6 gate-g2-relay scenes scene-7 scene-8 scene-9 scene-10 scene-11 scene-12 scene-13 testnet-plan-test
build-testnet: ## [testnet] Build the three wasm (token, agent, factory) with the testnet token, admin and MPC signer compiled in -> target/near-testnet
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-testnet
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.$(TESTNET_ROOT) AGENT_EXIT_ADMIN=$(TESTNET_ROOT) AGENT_EXIT_MPC=v1.signer-prod.testnet AGENT_EXIT_FACTORY=factory.$(TESTNET_ROOT) cargo near build non-reproducible-wasm --out-dir ../../target/near-testnet
	cd contracts/factory && cargo near build non-reproducible-wasm --out-dir ../../target/near-testnet

testnet-setup: ## [testnet] npm install for the G2 relay (chainsig.js, viem); network once
	cd scripts/testnet && npm install --no-audit --no-fund

deploy-testnet: ## [testnet] Create accounts, deploy xusd + translator + cheat, runner keys, lock translator, seed the fixed world [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) deploy
else
	@if [ -f contracts/res/SHA256SUMS ]; then \
	  echo "deploying the reproducible wasm in contracts/res (make build)"; \
	  (cd contracts/res && sha256sum -c SHA256SUMS) && \
	  built=$$(grep -a -o -E 'xusd\.[a-z0-9_.-]+\.testnet' contracts/res/agent_exit.wasm | sort -u | grep -x -F "xusd.$(TESTNET_ROOT)" || grep -a -o -E 'xusd\.[a-z0-9_.-]+\.testnet' contracts/res/agent_exit.wasm | head -1); \
	  if [ "$$built" != "xusd.$(TESTNET_ROOT)" ]; then \
	    echo "deploy-testnet: contracts/res/agent_exit.wasm has $$built compiled in, not xusd.$(TESTNET_ROOT). Run make build first." >&2; exit 1; fi; \
	  export AGENT_WASM=$(CURDIR)/contracts/res/agent_exit.wasm XUSD_WASM=$(CURDIR)/contracts/res/xusd.wasm && $(TESTNET_RUN) deploy; \
	else \
	  $(SUBMAKE) build-testnet && $(TESTNET_RUN) deploy; \
	fi
endif

seed-testnet: deploy-testnet ## [testnet] Same as deploy-testnet (every step skips what is done)

gates: ## [testnet] Gates G1 (key handover), G6 (NEAR AI signature on-chain), G2 (Chain Signatures Base) -> out/gates/ [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) gates
else
	$(TESTNET_RUN) gates
endif

gate-g1 gate-g2 gate-g6: gate-%: ## [testnet] One gate -> out/gates/ [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) $*
else
	$(TESTNET_RUN) $*
endif

gate-g2-relay: ## [testnet] Relay the last G2 signature on Base Sepolia (after make testnet-setup) [DRY_RUN=1]
	DRY_RUN=$(DRY_RUN) node scripts/testnet/g2-relay.mjs out/gates/g2-sign.json

scenes: ## [testnet] The six demo scenes in recording order 3 4 5 1 2 6 -> out/scenes/ [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) scenes
else
	$(TESTNET_RUN) scenes
endif

scene-7: ## [testnet] After scenes: summarizer lists translator at 1,000 for the AI buyer (make llm-buyer-testnet) -> out/scenes/scene-7.json [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) scene-7
else
	$(TESTNET_RUN) scene-7
endif

scene-8: ## [testnet] Invest: new agent translator3 (INVEST_AGENT), dave buys 20% of its earnings, 5 customers pay for 5 real translations, dave gets exactly 20% -> out/scenes/scene-8.json [DRY_RUN=1] (needs make build first; NEAR AI-signed if NEARAI_API_KEY has credit)
ifdef DRY_RUN
	@$(PLAN) scene-8
else
	$(TESTNET_RUN) scene-8
endif

scene-9: ## [testnet] Born clean: factory.ROOT creates born1 in one batch with no key (code hash pinned), the scan reports born_clean -> out/scenes/scene-9.json [DRY_RUN=1] (needs make build-testnet)
ifdef DRY_RUN
	@$(PLAN) scene-9
else
	$(TESTNET_RUN) scene-9
endif

scene-10: ## [testnet] Sealed prompt: fresh agent sealed1 sells at 100.00 with a committed config; the seller posts a FAKE bundle; the buyer's dispute decrypts it on-chain and refunds 100.00 -> out/scenes/scene-10.json [DRY_RUN=1] (needs make build first)
ifdef DRY_RUN
	@$(PLAN) scene-10
else
	$(TESTNET_RUN) scene-10
endif

scene-13: ## [testnet] The Exchange: up to six agents born clean in the factory (EXCHANGE_AGENTS=N; 3 recorded on agentexit5), each with its own revenue mix, share offer or sale listing, for the page's board -> out/scenes/scene-13.json [DRY_RUN=1] (after scene-9)
ifdef DRY_RUN
	@$(PLAN) scene-13
else
	$(TESTNET_RUN) scene-13
endif

scene-12: ## [testnet] An open share offer anyone can buy into from the page: alice offers 25% of the scene-8 agent's earnings in 10 parts of 2.5% at 25.00 xUSD-FAKE -> out/scenes/scene-12.json [DRY_RUN=1] (after scene-11)
ifdef DRY_RUN
	@$(PLAN) scene-12
else
	$(TESTNET_RUN) scene-12
endif

scene-11: ## [testnet] Business day on the scene-8 agent: the owner sets the constraints once, then customers pay, the runner key buys supplier jobs until the daily cap refuses, dave is paid in every payment -> out/scenes/scene-11.json [DRY_RUN=1] (after scene-8)
ifdef DRY_RUN
	@$(PLAN) scene-11
else
	$(TESTNET_RUN) scene-11
endif

testnet-plan-test: ## Offline: the testnet plan, guards and dry runs (no network)
	cargo test --quiet --locked --offline -p testnet-plan
	AGENT_EXIT_ENV=/nonexistent $(PLAN) deploy > /dev/null
	AGENT_EXIT_ENV=/nonexistent $(MAKE) -s scenes DRY_RUN=1 > /dev/null

.PHONY: attack-sandbox attack-testnet demo demo-setup record web-deploy e2e
attack-sandbox: ## (not built yet) Attack corpus against near-sandbox
	$(call not_built,attack-sandbox,M2 B2)
attack-testnet: ## (not built yet) [testnet] A1 A5 A17-A21 live
	$(call not_built,attack-testnet,M2 B2 on the owner's PC)
demo: mcp-build ## Offline story: quick verify subset, then the scripted agent-buys-agent replay, then how to open the page
	@mkdir -p $(OUT)
	@echo "== 1/3 verify subset: 76 checks replayed through the real contract code, compared with the goldens (the full unit/property run is make verify)"
	cargo run --quiet --locked --offline -p verify -- --json --out $(OUT)/verify.json --golden tests/golden/verify.json > /dev/null
	cargo run --quiet --locked --offline -p attack -- --out $(OUT)/attack.json --golden tests/golden/attack.json > /dev/null
	@echo "== 2/3 scripted replay: a fixed script (no AI model) plays the summarizer agent buying translator through the MCP tools"
	@node mcp/dist/scripts/agent-buys-agent.js --out $(OUT)/agent-buys-agent.json --golden tests/golden/agent-buys-agent.json > $(OUT)/agent-buys-agent.log 2>&1; s=$$?; grep -v "^agent-buys-agent: {" $(OUT)/agent-buys-agent.log; exit $$s
	@echo "== summary (full JSON in $(OUT)/)"
	@node scripts/demo-summary.mjs $(OUT)
	@echo "== 3/3 open the page"
	@echo "Run: make web   then open http://127.0.0.1:$(WEB_PORT)/   (recorded results; nothing here touches a chain)"
	@echo "Real testnet transactions (2026-11-10, agentexit5.testnet; G6 and the AI buyer on agentexit4.testnet): README section 'Proven on NEAR testnet' and the page's 'Run and verify' scenarios. A real AI model (Claude Haiku) buying on testnet: out/llm-buyer-testnet.json."
demo-setup: ## [testnet] Fresh demo world on a NEW root: reproducible build, deploy, G1, scenes, page data [DRY_RUN=1] [FORCE=1]
ifdef DRY_RUN
	@$(PLAN) deploy && $(PLAN) g1 && $(PLAN) scenes
else
	@used=$$(sed -n 's/.*"root": *"\([^"]*\)".*/\1/p' out/testnet/deploy.json 2>/dev/null | head -1); \
	if [ "$(TESTNET_ROOT)" = "$$used" ] && [ -z "$(FORCE)" ]; then \
	  echo "demo-setup: $(TESTNET_ROOT) already holds a played world (out/testnet/deploy.json): sales moved its deeds." >&2; \
	  echo "Put a new AGENT_EXIT_ROOT and AGENT_EXIT_ROOT_KEY in ~/.agent-exit.env (runbook section 2), or FORCE=1." >&2; exit 1; fi
	$(MAKE) build
	$(MAKE) deploy-testnet
	$(MAKE) gate-g1
	$(MAKE) scenes
	$(MAKE) web-testnet-data
	@echo "demo-setup: done on $(TESTNET_ROOT). Update the README testnet links from out/scenes, then commit out/ contracts/res web/data."
endif
record: ## (not built yet) [testnet] Save receipts to fixtures/recorded
	$(call not_built,record,M2 B3)
web-deploy: ## Hosted page: Vercel's Git integration deploys web/ on each push to main (no GitHub Actions)
	@echo "web-deploy: Vercel deploys web/ on each push to main: https://near-transparent-agent-market-web.vercel.app/"
	@echo "Settings: Root Directory web, Framework Other, no build command, Output Directory ."
e2e: ## (not built yet) [testnet] Playwright against the deployed page with a real wallet
	$(call not_built,e2e,M3 B5)

SCENES := 1 2 3 4 5 6
.PHONY: $(addprefix scene-,$(SCENES))
$(addprefix scene-,$(SCENES)): scene-%: ## [testnet] Demo scene N -> out/scenes/scene-N.json [DRY_RUN=1] [RECORDED=1 shows the recorded run offline]
ifdef DRY_RUN
	@$(PLAN) scene-$*
else ifdef RECORDED
	@test -f out/scenes/scene-$*.json || { echo "not recorded yet: out/scenes/scene-$*.json (run make scene-$* on testnet)" >&2; exit 1; }
	@node -e 'const s=require("./out/scenes/scene-$*.json"); console.log(`scene $${s.scene}: $${s.what}\npass: $${s.pass}`); for (const t of s.txs||[]) console.log(`  $${t.ok?"ok  ":"FAIL"} $${t.label}$${t.error?"  "+t.error.slice(0,160):""}$${t.explorer?"  "+t.explorer:""}`)'
else
	$(TESTNET_RUN) scene-$*
endif

GATES := g0 g3 g4
.PHONY: $(addprefix gate-,$(GATES))
$(addprefix gate-,$(GATES)): ## Day-1 gate scripts
	$(call not_built,$@,M1 gates)
