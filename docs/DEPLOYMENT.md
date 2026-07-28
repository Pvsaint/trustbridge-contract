# Deployment Guide

Step-by-step instructions for deploying **trustbridge-contract** to Stellar Testnet and Mainnet.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [ABI](ABI.md) · [Testnet Checklist](TESTNET_CHECKLIST.md) · [Security](SECURITY.md)

---

## Prerequisites

1. **Rust** ≥ 1.84 with `wasm32v1-none` target
2. **Stellar CLI** ≥ 26.x (recommended)
3. A funded Stellar account on the target network

```bash
rustup target add wasm32v1-none
curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
```

---

## Environment Variables

Copy [`.env.example`](../.env.example) to `.env` and configure:

| Variable | Required | Description |
|----------|----------|-------------|
| `NETWORK` | No | `testnet` (default), `mainnet`, or `futurenet` |
| `ADMIN` | **Yes** | G-address of contract admin |
| `SOURCE` | No | Stellar CLI identity name (default: `default`) |
| `ALIAS` | No | CLI contract alias (default: `trustbridge`) |
| `INIT` | No | Auto-initialize after deploy (default: `true`) |

---

## Testnet Deployment

### 1. Create a deployer identity

```bash
stellar keys generate deployer --network testnet --fund
stellar keys use deployer
export ADMIN=$(stellar keys address deployer)
```

The Friendbot funds testnet accounts automatically via `--fund`.

### 2. Build the contract

```bash
make build
# Output: target/wasm32v1-none/release/trustbridge-contract.wasm
```

### 3. Deploy and initialize

```bash
make deploy-testnet
# or:
NETWORK=testnet ADMIN=$ADMIN SOURCE=deployer ./scripts/deploy.sh
```

The script:

1. Builds WASM if missing
2. Runs `stellar contract deploy`
3. Calls `initialize(admin)`
4. Writes `deployments/testnet.json`

### 4. Verify deployment

```bash
export CONTRACT_ID=$(jq -r .contract_id deployments/testnet.json)

stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account deployer \
  --network testnet \
  -- get_stats
# Expected: { "total": 0, "verified": 0 }
```

---

## Testnet Checklist

A repeatable checklist to validate every release build against testnet. See [TESTNET_CHECKLIST.md](TESTNET_CHECKLIST.md) for the full numbered steps.

Required environment variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `NETWORK` | Yes | Must be `testnet` |
| `ADMIN` | Yes | G-address of the contract admin |
| `SOURCE` | Yes | Funded testnet CLI identity |
| `CONTRACT_ID` | After deploy | Recorded from `deployments/testnet.json` |

The checklist covers deploy → initialize → register → verify → export → remove, ending with cleanup. Defaults are safe: `NETWORK` defaults to `testnet`, so a mainnet run requires explicit configuration.

---

## Mainnet Deployment

### Dual-confirm checklist

Before every mainnet deploy, complete both confirmation steps:

1. **Build hash pin** — confirm the WASM hash matches the tagged release commit:
   ```bash
   sha256sum target/wasm32v1-none/release/trustbridge-contract.wasm
   ```
   Record the hash in your deploy runbook and verify it against the CI build artifact.

2. **Human confirmation** — set `CONFIRM_MAINNET=yes` to proceed:
   ```bash
   export CONFIRM_MAINNET=yes
   make deploy-mainnet
   ```
   The `deploy-mainnet` Makefile target refuses to run unless `CONFIRM_MAINNET` is set to `yes`. This prevents accidental mainnet invocations from a default `make` run.

### Post-deploy verification

After deployment, verify the contract is initialized and operational:

```bash
export CONTRACT_ID=$(jq -r .contract_id deployments/mainnet.json)

# Confirm the contract is initialized
stellar contract invoke \
  --id $CONTRACT_ID \
  --source_account deployer \
  --network mainnet \
  -- get_stats
# Expected: { "total": 0, "verified": 0 }

# Confirm the deployed WASM hash matches the pinned build hash
stellar contract get_wasm_hash \
  --id $CONTRACT_ID \
  --network mainnet
```

Checklist before mainnet:

- [ ] Admin address reviewed (prefer multisig)
- [ ] WASM built from a tagged release commit
- [ ] Build hash pinned and recorded
- [ ] `CONFIRM_MAINNET=yes` explicitly set
- [ ] `cargo test` and CI green on that commit
- [ ] Contract ID recorded in `deployments/mainnet.json`
- [ ] TTL extension plan documented for persistent entries

---

## Using the Makefile

| Target | Description |
|--------|-------------|
| `make deploy-testnet` | Build + deploy to testnet |
| `make deploy-mainnet` | Build + deploy to mainnet (requires `CONFIRM_MAINNET=yes`) |
| `make invoke-init` | Initialize an existing contract |
| `make invoke-register` | Register a username |
| `make invoke-lookup` | Read-only lookup |
| `make invoke-stats` | Read statistics |
| `make invoke-verify` | Verify a contributor (admin or verifier role) |
| `make invoke-revoke-verification` | Revoke verification (admin or verifier role) |
| `make testnet-checklist` | Run the testnet smoke checklist |

Example registration:

```bash
export CONTRACT_ID=C...
make invoke-register GITHUB_USER=octocat STELLAR_ADDR=G... SOURCE=deployer
```

---

## deploy.sh Reference

```bash
NETWORK=testnet \
ADMIN=GABC... \
SOURCE=deployer \
ALIAS=trustbridge \
INIT=true \
./scripts/deploy.sh
```

| Flag | Default | Description |
|------|---------|-------------|
| `NETWORK` | `testnet` | Target network |
| `ADMIN` | — | Required admin G-address |
| `SOURCE` | `default` | Signing identity |
| `ALIAS` | `trustbridge` | CLI alias for contract ID |
| `INIT` | `true` | Call `initialize` after deploy |

---

## Troubleshooting

### `wasm32v1-none` target not installed

```bash
rustup target add wasm32v1-none
```

### `wasm32-unknown-unknown` build fails on Rust 1.82+

`soroban-sdk` 26.x requires `wasm32v1-none`. Use `make build` (Stellar CLI) instead of legacy cargo target.

### `Unauthorized function call for address`

The `--source-account` must match the address that signed the auth payload. For `register`, source must own `stellar_address`. For `remove`, source must match `caller`.

### Insufficient fee / account not found

Ensure the source account is funded on the target network:

```bash
stellar keys fund deployer --network testnet
```

### Contract not initialized

Run initialize manually:

```bash
make invoke-init CONTRACT_ID=$CONTRACT_ID ADMIN=$ADMIN
```

---

## Post-Deployment

1. Publish the contract ID in the TrustBridge dashboard config
2. Configure the GitHub Action with `CONTRACT_ID` and `NETWORK`
3. Monitor events via a Stellar RPC endpoint or indexer
4. Schedule TTL extensions for persistent storage entries on long-lived networks

See [SECURITY.md](SECURITY.md) for operational security guidance.
