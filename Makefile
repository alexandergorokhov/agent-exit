# Agent Exit. Every target either does real work or prints "not built yet"
# and exits non-zero. Nothing here fakes success.
#
# Offline targets: verify, test, lint, audit (needs the advisory DB), hash.
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

.PHONY: golden
golden: ## Rewrite tests/golden/verify.json from the current code (review the diff!)
	cargo run --quiet --locked --offline -p verify -- --json --out tests/golden/verify.json
	cargo run --quiet --locked --offline -p attack -- --out tests/golden/attack.json > /dev/null

.PHONY: mcp-golden
mcp-golden: mcp-build ## Rewrite fixtures/mcp/world.json, tests/golden/agent-buys-agent.json and web/data/demo.json (review the diff!)
	cargo run --quiet --locked --offline -p verify --bin gen-mcp-world
	node mcp/dist/scripts/agent-buys-agent.js --out tests/golden/agent-buys-agent.json
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo

# ---------------------------------------------------------------- demo page (web/)

WEB_PORT ?= 8080
NODE_GLOBAL := $(shell npm root -g 2>/dev/null)

.PHONY: web-data
web-data: ## Rewrite web/data/demo.json from the contract (review the diff!)
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo

.PHONY: web
web: ## Serve the demo page on http://127.0.0.1:8080 (offline, recorded; ?network=testnet shows the unwired testnet mode)
	@command -v node >/dev/null || { echo "node not installed: the page server needs Node >= 20" >&2; exit 1; }
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo -- --check
	node web/serve.mjs $(WEB_PORT)

.PHONY: web-test
web-test: ## Playwright smoke test of the page (Chromium, desktop + 375px, no network)
	@command -v node >/dev/null || { echo "node not installed: web-test needs Node >= 20" >&2; exit 1; }
	cargo run --quiet --locked --offline -p verify --bin gen-web-demo -- --check
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

.PHONY: mcp-test
mcp-test: mcp-build ## MCP tests offline (S14, S15, E29, E30, schemas, skill) + agent-buys-agent vs its golden
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p verify --bin gen-mcp-world -- --check
	cd mcp && node --test --test-concurrency=4 'dist/test/*.test.js'
	node mcp/dist/scripts/agent-buys-agent.js --out $(OUT)/agent-buys-agent.json --golden tests/golden/agent-buys-agent.json
	$(MAKE) -s scripts-test

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

.PHONY: attack
attack: ## Seeded backdoor corpus + clean controls + contract refusals -> out/attack.json [N=200 SEED=42 CLEAN=50 FIXTURE=fixtures/judge-attack.json]
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p attack -- --n $(N) --seed $(SEED) --clean $(CLEAN) $(if $(FIXTURE),--fixture $(FIXTURE)) --out $(OUT)/attack.json > /dev/null

.PHONY: scan
scan: ## Scan one agent: make scan FILE=fixtures/scan/cheat.json (exit 0 clean, 2 blocked, 3 inconclusive)
	cargo run --quiet --locked --offline -p scan -- $(FILE)

FUZZ_CASES ?= 3000
.PHONY: fuzz
fuzz: ## Property-based fuzzing of outsider inputs: ft_on_transfer msg, NEAR AI proofs, set_limits args, MPC answers [FUZZ_CASES=3000]
	FUZZ_CASES=$(FUZZ_CASES) cargo test --locked --offline -p agent-exit fuzz_ -- --test-threads=4

.PHONY: lint
lint: ## rustfmt check + clippy -D warnings
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings

.PHONY: audit
audit: ## cargo audit (needs cargo-audit + advisory DB) and check ignored advisories stay out of the wasm
	@command -v cargo-audit >/dev/null || { echo "cargo-audit not installed: cargo install cargo-audit --locked" >&2; exit 1; }
	cargo audit
	./scripts/ci/wasm-deps-clean.sh

.PHONY: ci
ci: lint verify mcp-test ## What CI runs offline (audit runs as its own CI job)

.PHONY: build
build: ## Reproducible wasm build with cargo-near (Docker) -> contracts/res
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/xusd && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/res && sha256sum *.wasm > SHA256SUMS

.PHONY: build-dev
build-dev: ## Non-reproducible wasm build with cargo-near, ABI included (no Docker)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && cargo near build non-reproducible-wasm --out-dir ../../target/near
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near

.PHONY: hash
hash: ## Compare committed wasm with contracts/res/SHA256SUMS
	@test -f contracts/res/SHA256SUMS || { echo "not built yet: contracts/res/SHA256SUMS (run make build on a machine with Docker)" >&2; exit 1; }
	cd contracts/res && sha256sum -c SHA256SUMS

# ------------------------------------------------------------- not built yet

.PHONY: test-sandbox
test-sandbox: ## near-sandbox integration tests (S5, E5, A17, A21 on a real local node)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.sandbox AGENT_EXIT_ADMIN=xusd.sandbox cargo near build non-reproducible-wasm --out-dir ../../target/near-sandbox
	cd tests/sandbox && AGENT_WASM=../../target/near-sandbox/agent_exit.wasm XUSD_WASM=../../target/near-sandbox/xusd.wasm cargo test --locked -- --test-threads=1 --nocapture

# ------------------------------------------------------------- testnet day
# Runs on the owner's machine (NEAR RPC, Base Sepolia and NEAR AI Cloud are
# reachable there). Reads ~/.agent-exit.env (see .env.example); refuses
# anything but testnet. DRY_RUN=1 prints the plan offline and sends nothing.

TESTNET_ROOT ?= $(shell sed -n 's/^AGENT_EXIT_ROOT=//p' $(HOME)/.agent-exit.env 2>/dev/null | tail -1)
TESTNET_ROOT := $(if $(TESTNET_ROOT),$(TESTNET_ROOT),agentexit.testnet)
TESTNET_RUN = cd tools/testnet && cargo run --quiet --release --locked --
PLAN = cargo run --quiet --locked --offline -p testnet-plan --

.PHONY: build-testnet testnet-setup deploy-testnet seed-testnet gates gate-g1 gate-g2 gate-g6 gate-g2-relay scenes testnet-plan-test
build-testnet: ## [testnet] Build both wasm with the testnet token, admin and MPC signer compiled in -> target/near-testnet
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near-testnet
	cd contracts/agent && AGENT_EXIT_TOKEN=xusd.$(TESTNET_ROOT) AGENT_EXIT_ADMIN=$(TESTNET_ROOT) AGENT_EXIT_MPC=v1.signer-prod.testnet cargo near build non-reproducible-wasm --out-dir ../../target/near-testnet

testnet-setup: ## [testnet] npm install for the G2 relay (chainsig.js, viem); network once
	cd scripts/testnet && npm install --no-audit --no-fund

deploy-testnet: ## [testnet] Create accounts, deploy xusd + translator + cheat, runner keys, lock translator, seed the fixed world [DRY_RUN=1]
ifdef DRY_RUN
	@$(PLAN) deploy
else
	$(MAKE) build-testnet
	$(TESTNET_RUN) deploy
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

testnet-plan-test: ## Offline: the testnet plan, guards and dry runs (no network)
	cargo test --quiet --locked --offline -p testnet-plan
	AGENT_EXIT_ENV=/nonexistent $(PLAN) deploy > /dev/null
	AGENT_EXIT_ENV=/nonexistent $(MAKE) -s scenes DRY_RUN=1 > /dev/null

.PHONY: attack-sandbox attack-testnet demo demo-setup record web-deploy e2e
attack-sandbox: ## Attack corpus against near-sandbox
	$(call not_built,attack-sandbox,M2 B2)
attack-testnet: ## [testnet] A1 A5 A17-A21 live
	$(call not_built,attack-testnet,M2 B2 on the owner's PC)
demo: ## Whole demo in sandbox
	$(call not_built,demo,M3)
demo-setup: ## [testnet] Reset demo agents
	$(call not_built,demo-setup,M2 B3)
record: ## [testnet] Save receipts to fixtures/recorded
	$(call not_built,record,M2 B3)
web-deploy: ## [testnet] Vercel deploy
	$(call not_built,web-deploy,M3 B5)
e2e: ## [testnet] Playwright against the deployed page with a real wallet
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
