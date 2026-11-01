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
mcp-golden: mcp-build ## Rewrite fixtures/mcp/world.json and tests/golden/agent-buys-agent.json (review the diff!)
	cargo run --quiet --locked --offline -p verify --bin gen-mcp-world
	node mcp/dist/scripts/agent-buys-agent.js --out tests/golden/agent-buys-agent.json

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

.PHONY: agent-buys-agent
agent-buys-agent: mcp-build ## Scripted offline run: summarizer buys translator via MCP -> out/agent-buys-agent.json
	@mkdir -p $(OUT)
	node mcp/dist/scripts/agent-buys-agent.js --out $(OUT)/agent-buys-agent.json

.PHONY: attack
attack: ## Seeded backdoor corpus + clean controls + contract refusals -> out/attack.json [N=200 SEED=42 CLEAN=50 FIXTURE=fixtures/judge-attack.json]
	@mkdir -p $(OUT)
	cargo run --quiet --locked --offline -p attack -- --n $(N) --seed $(SEED) --clean $(CLEAN) $(if $(FIXTURE),--fixture $(FIXTURE)) --out $(OUT)/attack.json > /dev/null

.PHONY: scan
scan: ## Scan one agent: make scan FILE=fixtures/scan/cheat.json (exit 0 clean, 2 blocked, 3 inconclusive)
	cargo run --quiet --locked --offline -p scan -- $(FILE)

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

.PHONY: attack-sandbox attack-testnet fuzz demo deploy-testnet seed-testnet demo-setup record web-dev web-deploy e2e
attack-sandbox: ## Attack corpus against near-sandbox
	$(call not_built,attack-sandbox,M2 B2)
attack-testnet: ## [testnet] A1 A5 A17-A21 live
	$(call not_built,attack-testnet,M2 B2 on the owner's PC)
fuzz: ## cargo-fuzz targets
	$(call not_built,fuzz,M2 B2)
demo: ## Whole demo in sandbox
	$(call not_built,demo,M3)
deploy-testnet: ## [testnet] Deploy global contract, agents, xusd
	$(call not_built,deploy-testnet,M2 B3 on the owner's PC)
seed-testnet: ## [testnet] Seed the fixed world
	$(call not_built,seed-testnet,M2 B3 on the owner's PC)
demo-setup: ## [testnet] Reset demo agents
	$(call not_built,demo-setup,M2 B3)
record: ## [testnet] Save receipts to fixtures/recorded
	$(call not_built,record,M2 B3)
web-dev: ## Local hosted page
	$(call not_built,web-dev,M3 B5)
web-deploy: ## [testnet] Vercel deploy
	$(call not_built,web-deploy,M3 B5)
e2e: ## Playwright against the page
	$(call not_built,e2e,M3 B5)

SCENES := 1 2 3 4 5 6
.PHONY: $(addprefix scene-,$(SCENES))
$(addprefix scene-,$(SCENES)): ## Demo scene N (NETWORK=sandbox|testnet, RECORDED=1)
	$(call not_built,$@,M2 B3 / M3 B5)

GATES := g0 g1 g2 g3 g4 g6
.PHONY: $(addprefix gate-,$(GATES))
$(addprefix gate-,$(GATES)): ## Day-1 gate scripts
	$(call not_built,$@,M1 gates)
