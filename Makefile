# TrustBridge Contract — Makefile
#
# Common tasks for building, testing, and deploying the Soroban registry contract.
# Requires: Rust (≥ 1.84), wasm target, Stellar CLI (≥ 26.x recommended).

CRATE       := trustbridge-contract
WASM_V1     := target/wasm32v1-none/release/$(CRATE).wasm
WASM_LEGACY := target/wasm32-unknown-unknown/release/$(CRATE).wasm
STELLAR     ?= stellar
SOURCE      ?= default
NETWORK     ?= testnet
ADMIN       ?= $(shell $(STELLAR) keys address $(SOURCE) 2>/dev/null || echo "")
CONTRACT_ID ?=
GITHUB_USER ?=
STELLAR_ADDR ?=
BENCH_OUT   ?= bench-results.txt
NORM_BENCH_OUT ?= bench-username-normalization.txt
REGISTER_BUDGET_CPU_MAX ?= 25000000
REGISTER_BUDGET_MEM_MAX ?= 300000
BINDINGS_DIR ?= bindings/typescript
PKG_MANAGER  ?= pnpm

.PHONY: help build build-legacy test fuzz bench bench-export bench-username fmt lint docs docs-check check ci clean \
        deploy-testnet deploy-mainnet bindings bindings-build invoke-version require-contract-id \
	invoke-register invoke-lookup invoke-init invoke-stats install-target invoke-extend-ttl \
	bench-register-budget

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

install-target: ## Install wasm compilation targets
	rustup target add wasm32v1-none wasm32-unknown-unknown

build: install-target ## Build optimized WASM via Stellar CLI (recommended)
	$(STELLAR) contract build

build-legacy: install-target ## Build with cargo directly (wasm32-unknown-unknown)
	cargo build --target wasm32-unknown-unknown --release

test: ## Run unit tests
	cargo test

fuzz: ## Run the invariant property fuzzing suite (deterministic seeds)
	cargo test fuzz -- --nocapture

bench: ## Report CPU/memory cost per contract operation
	cargo test bench -- --nocapture --test-threads=1

bench-export: ## Write export CPU benchmark results to $(BENCH_OUT)
	cargo test test_bench_export -- --nocapture --test-threads=1 | tee $(BENCH_OUT)
	@echo "Benchmark results written to $(BENCH_OUT)"

bench-username: ## Write username case-normalization benchmark results to $(NORM_BENCH_OUT)
	cargo test test_bench_username_case_normalization -- --nocapture --test-threads=1 | tee $(NORM_BENCH_OUT)
	@echo "Benchmark results written to $(NORM_BENCH_OUT)"

bench-register-budget: ## Validate register cost stays under CPU/memory thresholds (baseline + max-length username)
	@echo "Running register budget sampling (CPU<=$(REGISTER_BUDGET_CPU_MAX), MEM<=$(REGISTER_BUDGET_MEM_MAX))"
	@cargo test test_report_register_budget_samples -- --nocapture --test-threads=1 | \
	awk -F',' -v cpu_max=$(REGISTER_BUDGET_CPU_MAX) -v mem_max=$(REGISTER_BUDGET_MEM_MAX) '\
	BEGIN { baseline=0; stressed=0; failed=0 } \
	/^register,(baseline|max_username_len),/ { \
	  input=$$2; cpu=$$3+0; mem=$$4+0; \
	  if (input=="baseline") baseline=1; \
	  if (input=="max_username_len") stressed=1; \
	  if (cpu > cpu_max || mem > mem_max) { \
	    failed=1; \
	    printf("ERROR: register budget exceeded for input=%s (cpu=%d, mem=%d, limits cpu<=%d mem<=%d)\n", input, cpu, mem, cpu_max, mem_max); \
	  } \
	} \
	END { \
	  if (!baseline || !stressed) { \
	    print "ERROR: register budget output missing baseline or max_username_len sample"; \
	    exit 2; \
	  } \
	  if (failed) exit 1; \
	  print "OK: register budget samples are within configured thresholds"; \
	}'

fmt: ## Check formatting
	cargo fmt --all -- --check

lint: ## Run clippy
	cargo clippy --all-targets -- -D warnings

docs: ## Build rustdoc for public API (opens in browser)
	cargo doc --no-deps --open

docs-check: ## Build rustdoc without opening browser (CI-equivalent)
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

check: fmt lint test build docs-check ## Run full local quality gate

wasm-size: build ## Report release WASM size and check against budget (WASM_SIZE_LIMIT)
	@if [ -f $(WASM_V1) ]; then \
		WASM=$(WASM_V1); \
	elif [ -f $(WASM_LEGACY) ]; then \
		WASM=$(WASM_LEGACY); \
	else \
		echo "ERROR: No WASM artifact found. Run 'make build' first."; exit 1; \
	fi; \
	SIZE=$$(wc -c < "$$WASM"); \
	LIMIT=$(WASM_SIZE_LIMIT); \
	LIMIT_KB=$$(( LIMIT / 1024 )); \
	SIZE_KB=$$(( SIZE / 1024 )); \
	echo "──────────────────────────────────────────"; \
	echo "  WASM size report"; \
	echo "──────────────────────────────────────────"; \
	echo "  File   : $$WASM"; \
	echo "  Size   : $$SIZE bytes (~$${SIZE_KB} KB)"; \
	echo "  Limit  : $$LIMIT bytes ($${LIMIT_KB} KB)"; \
	echo "──────────────────────────────────────────"; \
	if [ "$$SIZE" -gt "$$LIMIT" ]; then \
		echo ""; \
		echo "FAIL: WASM size $$SIZE bytes exceeds budget $$LIMIT bytes (over by $$(( SIZE - LIMIT )) bytes)"; \
		echo "Raise WASM_SIZE_LIMIT in Makefile and .github/workflows/ci.yml if growth is intentional."; \
		exit 1; \
	else \
		echo "  Headroom: $$(( LIMIT - SIZE )) bytes remaining"; \
		echo ""; \
		echo "PASS: WASM size is within budget."; \
	fi

check: fmt lint test build wasm-size ## Run full local quality gate

ci: check ## Alias for CI-equivalent checks (fmt + lint + test + build + wasm-size)

clean: ## Remove build artifacts
	cargo clean
	rm -rf target/wasm32v1-none target/wasm32-unknown-unknown $(BINDINGS_DIR)

bindings: ## Generate the TypeScript bindings package (CONTRACT_ID required)
	@if [ -z "$(CONTRACT_ID)" ]; then \
		echo "Set CONTRACT_ID=<C...> to generate bindings."; exit 1; \
	fi
	$(STELLAR) contract bindings typescript \
		--network $(NETWORK) \
		--contract-id $(CONTRACT_ID) \
		--output-dir $(BINDINGS_DIR) \
		--overwrite

bindings-build: bindings ## Generate and build the TypeScript bindings package
	cd $(BINDINGS_DIR) && $(PKG_MANAGER) install && $(PKG_MANAGER) run build

deploy-testnet: build ## Deploy to Stellar Testnet
	NETWORK=testnet ADMIN=$(ADMIN) ./scripts/deploy.sh

deploy-mainnet: build ## Deploy to Stellar Mainnet (requires explicit ADMIN and CONFIRM_MAINNET=yes)
	@if [ -z "$(ADMIN)" ]; then echo "Set ADMIN to the G-address of the contract admin."; exit 1; fi
	@if [ "$(CONFIRM_MAINNET)" != "yes" ]; then echo "ERROR: CONFIRM_MAINNET=yes is required for mainnet deployment to prevent accidental mainnet deploys."; exit 1; fi
	NETWORK=mainnet ADMIN=$(ADMIN) ./scripts/deploy.sh

require-contract-id:
	@if [ -z "$(CONTRACT_ID)" ]; then \
		echo "ERROR: set CONTRACT_ID=<C...> for this target."; exit 1; \
	fi

invoke-init: require-contract-id ## Initialize contract (CONTRACT_ID and ADMIN required)
	@if [ -z "$(ADMIN)" ]; then \
		echo "ERROR: set ADMIN to the G-address of the contract admin."; exit 1; \
	fi
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- initialize --admin $(ADMIN)

invoke-register: require-contract-id ## Register a GitHub username (GITHUB_USER, STELLAR_ADDR, CONTRACT_ID)
	@if [ -z "$(GITHUB_USER)" ]; then \
		echo "ERROR: set GITHUB_USER=<username> for this target."; exit 1; \
	fi
	@if [ -z "$(STELLAR_ADDR)" ]; then \
		echo "ERROR: set STELLAR_ADDR=<G...> for this target."; exit 1; \
	fi
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- register \
		--github-username $(GITHUB_USER) \
		--stellar-address $(STELLAR_ADDR)

invoke-lookup: require-contract-id ## Look up a GitHub username (read-only simulation)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- get_address --github-username $(GITHUB_USER)

invoke-version: require-contract-id ## Read the deployed contract version (read-only)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- version

invoke-stats: require-contract-id ## Read registry statistics (read-only)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- get_stats

invoke-verify: ## Mark a contributor as verified (admin-only) (GITHUB_USER, SOURCE=admin, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- verify --github-username $(GITHUB_USER)

BULK_REVOKE_FILE ?= usernames.txt
BULK_REVOKE_LOG  ?= bulk-revoke-audit.log

bulk-revoke-dry-run: require-contract-id ## Dry-run bulk revoke from BULK_REVOKE_FILE (no transactions submitted)
	@echo "=== Dry-run bulk revoke from $(BULK_REVOKE_FILE) ==="
	@bash scripts/bulk_revoke.sh \
		--file $(BULK_REVOKE_FILE) \
		--contract $(CONTRACT_ID) \
		--source $(SOURCE) \
		--network $(NETWORK) \
		--dry-run

bulk-revoke: require-contract-id ## Bulk revoke from BULK_REVOKE_FILE with audit log (--yes skips confirm, add CONFIRM=yes for mainnet)
	@bash scripts/bulk_revoke.sh \
		--file $(BULK_REVOKE_FILE) \
		--contract $(CONTRACT_ID) \
		--source $(SOURCE) \
		--network $(NETWORK) \
		--audit-log $(BULK_REVOKE_LOG) \
		--continue-on-error \
		$(if $(filter yes,$(CONFIRM)),--yes,)

invoke-revoke-verification: ## Revoke contributor verification (admin-only) (GITHUB_USER, SOURCE=admin, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- revoke_verification --github-username $(GITHUB_USER)

invoke-get-all-registered: ## Export full registry mapping (admin-only) (SOURCE=admin, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- get_all_registered

invoke-export-paginated: ## Export paginated records with cursor (admin-only) (CURSOR, LIMIT, SOURCE=admin, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- get_registered_paginated --cursor $(CURSOR) --limit $(LIMIT)

invoke-public-paginated: ## Public paginated read for indexer/dashboard (CURSOR, LIMIT, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- get_public_paginated --cursor $(CURSOR) --limit $(LIMIT)

invoke-remove: ## Remove a registration (CALLER, GITHUB_USER, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- remove --caller $(CALLER) --github-username $(GITHUB_USER)

invoke-set-paused: ## Toggle contract pause state (PAUSED, SOURCE=admin, CONTRACT_ID)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		--send=yes \
		-- set_paused --paused $(PAUSED)

# ── Simulate-register gas reporting (Issue #111) ─────────────────────────────
#
# These targets wrap `stellar contract invoke ... simulate` for the `register`
# function and print resource/fee fields WITHOUT spending funds or submitting
# a transaction.  They are the baseline gas-reporting tool for Wave invoke budgets.
#
# Usage:
#   make simulate-register CONTRACT_ID=C... GITHUB_USER=octocat STELLAR_ADDR=G...
#   make simulate-register-max CONTRACT_ID=C... STELLAR_ADDR=G...
#   make simulate-register-compare CONTRACT_ID=C... STELLAR_ADDR=G...
#
# Required variables:
#   CONTRACT_ID    – deployed contract address (C...)
#   STELLAR_ADDR   – the Stellar G-address to register
#   SOURCE         – Stellar CLI identity to sign the simulation (default: default)
#   NETWORK        – testnet | mainnet (default: testnet)
#
# Optional variables:
#   GITHUB_USER    – username for baseline simulation (default: octocat)
#   MAX_GITHUB_USER – 39-char username for max-length simulation

GITHUB_USER      ?= octocat
MAX_GITHUB_USER  ?= aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
SIMULATE_OUT     ?= simulate-register-results.txt

.PHONY: simulate-register simulate-register-max simulate-register-compare \
        require-stellar-addr

require-stellar-addr:
	@if [ -z "$(STELLAR_ADDR)" ]; then \
		echo "ERROR: set STELLAR_ADDR=<G...> for this target."; exit 1; \
	fi

## Simulate register with a baseline short username (no --send, no fees spent).
## Prints resource fields: cpu_instructions, memory_bytes, min_resource_fee.
simulate-register: require-contract-id require-stellar-addr ## Simulate register (baseline username, no funds spent)
	@echo "=== simulate-register: baseline username '$(GITHUB_USER)' ==="
	@echo "Network: $(NETWORK)  Contract: $(CONTRACT_ID)"
	@echo "NOTE: simulation only — no transaction submitted, no fees charged."
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- register \
		--github-username $(GITHUB_USER) \
		--stellar-address $(STELLAR_ADDR)
	@echo ""
	@echo "Interpretation:"
	@echo "  cpu_instructions  – metered Wasm CPU cost for this invocation"
	@echo "  mem_bytes         – metered memory footprint in bytes"
	@echo "  min_resource_fee  – minimum fee in stroops (1 XLM = 10 000 000 stroops)"
	@echo "  See docs/DEPLOYMENT.md#simulate-register for field definitions."

## Simulate register with the maximum-length username (39 chars, no --send).
## Compare output against simulate-register to measure username-length impact on cost.
simulate-register-max: require-contract-id require-stellar-addr ## Simulate register with max-length username (39 chars)
	@echo "=== simulate-register: max-length username (39 chars) ==="
	@echo "Network: $(NETWORK)  Contract: $(CONTRACT_ID)"
	@echo "NOTE: simulation only — no transaction submitted, no fees charged."
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- register \
		--github-username $(MAX_GITHUB_USER) \
		--stellar-address $(STELLAR_ADDR)
	@echo ""
	@echo "Interpretation: compare cpu_instructions and min_resource_fee"
	@echo "  against simulate-register (baseline) to see the username-length delta."

## Run both baseline and max-length simulations and write results to $(SIMULATE_OUT).
## Useful for diffing across branches or contract versions.
simulate-register-compare: require-contract-id require-stellar-addr ## Compare baseline vs max-length simulate-register, write to $(SIMULATE_OUT)
	@echo "=== simulate-register: baseline vs max-length comparison ===" | tee $(SIMULATE_OUT)
	@echo "Network: $(NETWORK)  Contract: $(CONTRACT_ID)" | tee -a $(SIMULATE_OUT)
	@echo "Date: $$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a $(SIMULATE_OUT)
	@echo "" | tee -a $(SIMULATE_OUT)
	@echo "--- baseline (username: '$(GITHUB_USER)') ---" | tee -a $(SIMULATE_OUT)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- register \
		--github-username $(GITHUB_USER) \
		--stellar-address $(STELLAR_ADDR) \
		2>&1 | tee -a $(SIMULATE_OUT)
	@echo "" | tee -a $(SIMULATE_OUT)
	@echo "--- max-length (username: 39 chars) ---" | tee -a $(SIMULATE_OUT)
	$(STELLAR) contract invoke \
		--id $(CONTRACT_ID) \
		--source-account $(SOURCE) \
		--network $(NETWORK) \
		-- register \
		--github-username $(MAX_GITHUB_USER) \
		--stellar-address $(STELLAR_ADDR) \
		2>&1 | tee -a $(SIMULATE_OUT)
	@echo "" | tee -a $(SIMULATE_OUT)
	@echo "Results written to $(SIMULATE_OUT)"
	@echo "See docs/DEPLOYMENT.md#simulate-register for interpretation guide."
