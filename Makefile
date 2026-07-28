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
BINDINGS_DIR ?= bindings/typescript
PKG_MANAGER  ?= pnpm

.PHONY: help build build-legacy test fuzz bench bench-export bench-username fmt lint docs docs-check check ci clean \
        deploy-testnet deploy-mainnet bindings bindings-build invoke-version require-contract-id \
        invoke-register invoke-lookup invoke-init invoke-stats install-target invoke-extend-ttl

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

fmt: ## Check formatting
	cargo fmt --all -- --check

lint: ## Run clippy
	cargo clippy --all-targets -- -D warnings

docs: ## Build rustdoc for public API (opens in browser)
	cargo doc --no-deps --open

docs-check: ## Build rustdoc without opening browser (CI-equivalent)
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

check: fmt lint test build docs-check ## Run full local quality gate

ci: check ## Alias for CI-equivalent checks

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

deploy-mainnet: build ## Deploy to Stellar Mainnet (requires explicit ADMIN)
	@if [ -z "$(ADMIN)" ]; then echo "Set ADMIN to the G-address of the contract admin."; exit 1; fi
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
