# Agent Exit. Every target either does real work or prints "not built yet"
# and exits non-zero. Nothing here fakes success.
#
# Offline targets: verify, test, lint, audit (needs the advisory DB), hash.
# Testnet targets are marked [testnet]; they never touch mainnet.

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

PROP_CASES ?= 10000
OUT := out

define not_built
	@echo "not built yet: $(1) (planned: $(2))" >&2; exit 1
endef

.PHONY: help
help: ## List targets
	@grep -E '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/' | sort

# ---------------------------------------------------------------- offline core

.PHONY: setup
setup: ## Print toolchain versions; fetch crates (network once)
	rustc --version
	cargo --version
	cargo fetch --locked

.PHONY: test
test: ## Unit + property tests (offline)
	PROP_CASES=$(PROP_CASES) cargo test --workspace --locked --offline

.PHONY: verify
verify: ## Offline rerun: tests + fixed world -> out/verify.json, compared with tests/golden/verify.json
	@mkdir -p $(OUT)
	PROP_CASES=$(PROP_CASES) cargo test --workspace --locked --offline --quiet
	cargo run --quiet --locked --offline -p verify -- --json --out $(OUT)/verify.json --golden tests/golden/verify.json

.PHONY: golden
golden: ## Rewrite tests/golden/verify.json from the current code (review the diff!)
	cargo run --quiet --locked --offline -p verify -- --json --out tests/golden/verify.json

.PHONY: lint
lint: ## rustfmt check + clippy -D warnings
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings

.PHONY: audit
audit: ## cargo audit (needs cargo-audit and the advisory DB)
	@command -v cargo-audit >/dev/null || { echo "cargo-audit not installed: cargo install cargo-audit --locked" >&2; exit 1; }
	cargo audit

.PHONY: ci
ci: lint verify ## What CI runs offline (audit runs as its own CI job)

.PHONY: build
build: ## Reproducible wasm build with cargo-near (Docker) -> contracts/res
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/xusd && cargo near build reproducible-wasm --out-dir ../res
	cd contracts/res && sha256sum *.wasm > SHA256SUMS

.PHONY: build-dev
build-dev: ## Non-reproducible wasm build with cargo-near (no Docker)
	@command -v cargo-near >/dev/null || { echo "cargo-near not installed: cargo install cargo-near --locked" >&2; exit 1; }
	cd contracts/agent && cargo near build non-reproducible-wasm --out-dir ../../target/near
	cd contracts/xusd && cargo near build non-reproducible-wasm --out-dir ../../target/near

.PHONY: hash
hash: ## Compare committed wasm with contracts/res/SHA256SUMS
	@test -f contracts/res/SHA256SUMS || { echo "not built yet: contracts/res/SHA256SUMS (run make build on a machine with Docker)" >&2; exit 1; }
	cd contracts/res && sha256sum -c SHA256SUMS

# ------------------------------------------------------------- not built yet

.PHONY: test-sandbox attack attack-sandbox attack-testnet fuzz demo deploy-testnet seed-testnet demo-setup record mcp web-dev web-deploy e2e
test-sandbox: ## near-sandbox integration tests
	$(call not_built,test-sandbox,M2 B1 sandbox harness; runs on a machine where the near-sandbox binary downloads)
attack: ## Seeded attack corpus (A1-A16 + 50 clean agents)
	$(call not_built,attack,M2 B2)
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
mcp: ## MCP server (stdio)
	$(call not_built,mcp,M3 B4)
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
