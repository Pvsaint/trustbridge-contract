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

## Simulate-Register Gas Reporting

Operators can estimate `register` resource costs **before** committing funds, using the
`stellar contract invoke` simulation path (no `--send=yes`).  This is the recommended way
to set Wave invoke budgets before contributors hit the contract at scale.

> **Works without spending funds.**  Simulation runs locally against the current ledger state.
> No transaction is submitted and no fees are charged.

### Makefile targets

| Target | Description |
|--------|-------------|
| `make simulate-register` | Baseline: short username (`octocat` by default) |
| `make simulate-register-max` | Max-length: 39-character username |
| `make simulate-register-compare` | Both runs back-to-back, output to `simulate-register-results.txt` |

**Prerequisites:**  `CONTRACT_ID` and `STELLAR_ADDR` must be set.  The `SOURCE` account just
needs to exist on the network — it is not charged.

```bash
export CONTRACT_ID=C...
export STELLAR_ADDR=G...   # the address that *would* be registered

# Baseline simulation
make simulate-register CONTRACT_ID=$CONTRACT_ID STELLAR_ADDR=$STELLAR_ADDR

# Max-length username
make simulate-register-max CONTRACT_ID=$CONTRACT_ID STELLAR_ADDR=$STELLAR_ADDR

# Compare both and write to file
make simulate-register-compare CONTRACT_ID=$CONTRACT_ID STELLAR_ADDR=$STELLAR_ADDR
```

Or call the CLI directly:

```bash
# Simulate register — no --send, no fees spent
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account deployer \
  --network testnet \
  -- register \
  --github-username octocat \
  --stellar-address $STELLAR_ADDR
```

### Output fields

The CLI prints a JSON-like block with at least these resource fields:

| Field | Description |
|-------|-------------|
| `cpu_instructions` | Metered Wasm CPU cost for this invocation |
| `mem_bytes` | Metered memory footprint in bytes |
| `min_resource_fee` | Minimum fee in stroops (1 XLM = 10 000 000 stroops) |
| `read_bytes` | Bytes read from ledger entries |
| `write_bytes` | Bytes written to ledger entries |

**Sample output interpretation (approximate, testnet only):**

```
Simulation result:
  cpu_instructions: 1_234_567
  mem_bytes:        45_678
  min_resource_fee: 9_876  stroops  (~0.001 XLM)
```

A `min_resource_fee` of ~10 000 stroops means each `register` call costs roughly 0.001 XLM
at the simulated ledger state.  Multiply by the expected number of Wave registrations to budget
the total fee pool.

### Baseline vs. max-length comparison

Issue #111 calls for comparing baseline (short username) against the maximum-length (39-char)
username to measure the username-length delta on `cpu_instructions` and `min_resource_fee`.

Run the comparison and diff the results:

```bash
make simulate-register-compare CONTRACT_ID=$CONTRACT_ID STELLAR_ADDR=$STELLAR_ADDR
# Output written to simulate-register-results.txt
diff <(git show HEAD:simulate-register-results.txt) simulate-register-results.txt
```

### Limitations

| Limitation | Impact |
|------------|--------|
| Simulation ≠ live execution | Fee shown is valid at simulation time; live fees can differ under load |
| `min_resource_fee` is a floor | Actual fee may be higher if ledger load is elevated |
| Auth simulation | The `--source-account` is simulated but not the registrant; fees are correct but the call would fail on a live network if `stellar_address` doesn't match `source-account` |
| No ledger commit | `count` and index updates are computed but not persisted; a re-simulation of a second `register` will show the same cost as the first |
| Rent fees change with upgrades | Re-simulate after protocol or fee schedule upgrades |

See [STORAGE_RENT.md](STORAGE_RENT.md) for how simulation fits into the broader rent estimation workflow.

---

## Post-Deployment

1. Publish the contract ID in the TrustBridge dashboard config
2. Configure the GitHub Action with `CONTRACT_ID` and `NETWORK`
3. Monitor events via a Stellar RPC endpoint or indexer
4. Schedule TTL extensions for persistent storage entries on long-lived networks

See [SECURITY.md](SECURITY.md) for operational security guidance.
