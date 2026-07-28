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

---

## WASM Size Budget

Soroban charges upload fees proportional to WASM size, and the protocol imposes
an upper limit. Keeping the binary small reduces deploy cost and makes upgrades
cheaper.

### Current budget

| Metric | Value |
|--------|-------|
| Hard limit (CI gate) | **200 KB** (204 800 bytes) |
| Typical release size | ~85 KB |
| Headroom | ~115 KB |

The hard limit is enforced by:

- **CI**: the _WASM size regression gate_ step in `.github/workflows/ci.yml` fails
  the build when `trustbridge-contract.wasm` exceeds `WASM_SIZE_LIMIT`.
- **Local**: `make wasm-size` runs the same check after `make build`.

### How to measure locally

```bash
make wasm-size
```

Output:

```
──────────────────────────────────────────
  WASM size report
──────────────────────────────────────────
  File   : target/wasm32v1-none/release/trustbridge-contract.wasm
  Size   : 87040 bytes (~85 KB)
  Limit  : 204800 bytes (200 KB)
──────────────────────────────────────────
  Headroom: 117760 bytes remaining

PASS: WASM size is within budget.
```

### Rationale for the 200 KB limit

The optimised release WASM currently sits near 85 KB. 200 KB provides ~115 KB
of headroom for intentional feature additions while still catching accidental
bloat — e.g. a new dependency that pulls in an unintended transitive crate, or
a build profile misconfiguration that disables LTO.

### How to raise the limit

If intentional feature growth pushes the binary past 200 KB:

1. Run `make wasm-size` locally to measure the new size.
2. Round up to the nearest 10 KB and add ~20 KB of headroom to get the new
   ceiling.
3. Update `WASM_SIZE_LIMIT` in **both** places (they must stay in sync):
   - `Makefile` — the `WASM_SIZE_LIMIT ?=` variable
   - `.github/workflows/ci.yml` — the `WASM_SIZE_LIMIT:` env variable
4. Document the new limit and the feature that required the bump in this table.
5. Include before/after sizes in the PR description.
