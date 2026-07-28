# Deployment Guide

Step-by-step instructions for deploying **trustbridge-contract** to Stellar Testnet and Mainnet.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [ABI](ABI.md)

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

## Mainnet Deployment

Mainnet deployment follows the same flow with additional safeguards:

```bash
# Use a dedicated ops identity — never reuse testnet keys
stellar keys generate trustbridge-ops --network mainnet
# Fund manually via exchange or existing account

export ADMIN=G...   # Consider a multisig address
export SOURCE=trustbridge-ops

make deploy-mainnet
```

**Checklist before mainnet:**

- [ ] Admin address reviewed (prefer multisig)
- [ ] WASM built from a tagged release commit
- [ ] `cargo test` and CI green on that commit
- [ ] Contract ID recorded in `deployments/mainnet.json`
- [ ] TTL extension plan documented for persistent entries

---

## Using the Makefile

| Target | Description |
|--------|-------------|
| `make deploy-testnet` | Build + deploy to testnet |
| `make deploy-mainnet` | Build + deploy to mainnet |
| `make invoke-init` | Initialize an existing contract |
| `make invoke-register` | Register a username |
| `make invoke-lookup` | Read-only lookup |
| `make invoke-stats` | Read statistics |
| `make export-registry` | Export the full registry to JSON (see [Registry Export & Import](#registry-export--import)) |
| `make validate-registry` | Validate an export file against live state, no writes |

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

## Registry Export & Import

Two operator scripts cover backups, dashboard migrations, and audit snapshots
of the registry, without giving up the on-chain data as the source of truth.

### Export (Issue #132)

`scripts/export_registry.sh` pages through the admin-only
`get_registered_paginated` and writes a single JSON file with a stable schema.

| Variable | Required | Description |
|----------|----------|--------------|
| `CONTRACT_ID` | **Yes** | Deployed contract ID |
| `SOURCE` | **Yes** | Stellar CLI identity of the contract admin — `get_registered_paginated` is admin-gated |
| `NETWORK` | No | `testnet` (default), `mainnet`, or `futurenet` |
| `OUTPUT_FILE` | No | Output path (default: `registry-export-<network>.json`) |
| `PAGE_LIMIT` | No | Records per page (default: `100`, the contract's `MAX_PAGE_LIMIT`) |

```bash
CONTRACT_ID=$CONTRACT_ID SOURCE=admin NETWORK=testnet ./scripts/export_registry.sh
# or:
make export-registry CONTRACT_ID=$CONTRACT_ID SOURCE=admin
```

The script fails with a clear error and a non-zero exit code if `CONTRACT_ID`,
`SOURCE`, the Stellar CLI, or `jq` are missing.

**Output schema** (stable field names for dashboard/indexer consumers):

```json
{
  "schema_version": 1,
  "contract_id": "C...",
  "network": "testnet",
  "exported_at": "2026-01-01T00:00:00Z",
  "count": 2,
  "records": [
    {
      "github_username": "octocat",
      "stellar_address": "G...",
      "verified": true,
      "registered_at": 1732800000
    }
  ]
}
```

### Import / validate (Issue #133)

Import does not bypass on-chain auth. `scripts/validate_registry.sh` never
writes to the contract — it validates an export file against live state,
which covers staging restores and migration dry-runs.

| Variable | Required | Description |
|----------|----------|--------------|
| `CONTRACT_ID` | **Yes** | Deployed contract ID |
| `NETWORK` | No | `testnet` (default), `mainnet`, or `futurenet` |
| `SOURCE` | No | Identity for per-record reads (default: `default`); `get_address` needs no auth, so any funded identity works |
| `ADMIN_SOURCE` | No | Admin identity; when set, also detects on-chain registrations **missing from** the export via admin-gated `get_registered_paginated` |
| `PAGE_LIMIT` | No | Records per page for the admin-side check (default: `100`) |

```bash
CONTRACT_ID=$CONTRACT_ID NETWORK=testnet ./scripts/validate_registry.sh registry-export-testnet.json
# or, for the full two-way diff:
make validate-registry CONTRACT_ID=$CONTRACT_ID ADMIN_SOURCE=admin EXPORT_FILE=registry-export-testnet.json
```

The script reports every mismatch it finds and exits `1` if any are present,
`0` if the export matches live state exactly, `2` on a usage or config error
(missing file, malformed JSON, missing `CONTRACT_ID`, etc.):

| Diff type | Meaning |
|-----------|---------|
| `MISSING_ONCHAIN` | Export has the username; the contract does not |
| `ADDRESS_MISMATCH` | `stellar_address` differs between export and chain |
| `VERIFIED_MISMATCH` | `verified` flag differs between export and chain |
| `MISSING_FROM_EXPORT` | Contract has the username; the export file does not (`ADMIN_SOURCE` only) |

**Safety warning:** this tool is validate-only by design. It does not replay
writes. Never use an export file to blindly overwrite mainnet state —
`register`/`verify`/`revoke_verification` all require the appropriate signer
to authorize each call individually, so a "replay" is a reviewed, one-by-one
series of ordinary invocations (e.g. `make invoke-register`), not a bulk
import. Treat `scripts/validate_registry.sh` output as the checklist for that
review, run it against testnet first, and never against mainnet without a
human reading every reported diff.

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
