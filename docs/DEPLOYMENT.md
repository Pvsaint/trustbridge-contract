# Deployment Guide

Step-by-step instructions for deploying **trustbridge-contract** to Stellar Testnet and Mainnet.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [ABI](ABI.md) · [CONTRACT_HEALTH](CONTRACT_HEALTH.md)

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

## Upgrade Window Read-Only Mode

When rotating the WASM hash, put the contract into pause mode first so the
upgrade window behaves as read-only for integrators:

1. Call `set_paused(true)` as admin.
2. Publish or apply the new WASM upgrade.
3. Verify the new binary with the existing upgrade checks in [ABI.md](ABI.md)
  and the deployment script flow in [scripts/deploy.sh](../scripts/deploy.sh).
4. Call `set_paused(false)` once the upgrade is confirmed healthy.

During this window, lookups remain safe, but mutation entry points reject with
the existing pause error. In practice that means dashboards and indexers can
keep using `get_address`, `get_stats`, and the export/pagination reads, while
`register`, `remove`, `verify`, `pause`, `unpause`, `set_role`, `remove_role`,
`set_cooldown`, `attest_upgrade`, `clear_attestation`, and `upgrade` are
expected to fail fast until the contract is unpaused.

This mode is an operator procedure, not a new ABI surface, so it does not
change the public contract interface.

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
| `make demo-e2e` | Run the cross-repo E2E demo (register → verify → lookup → export) |

Example registration:

```bash
export CONTRACT_ID=C...
make invoke-register GITHUB_USER=octocat STELLAR_ADDR=G... SOURCE=deployer
```

Equivalent raw CLI invocation:

```bash
stellar contract invoke --id $CONTRACT_ID \
  --source-account deployer \
  --network testnet \
  --send=yes \
  -- register \
  --github-username octocat \
  --stellar-address G...
```

`register` requires the source account to authenticate as `stellar-address`. If the
username is already registered to a different address, that previous address must
also sign. See [ABI.md](ABI.md#register) for full auth requirements and failure modes.

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

## Contract Upgrades & Schema Migrations

During a contract upgrade, operators must maintain the integrity of the contract schema. Upgrade harnesses and runbooks depend on a single source of truth for migration state on-chain.

### 1. Verify Current Schema Version
To check the current deployed version before upgrading:
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account deployer \
  --network testnet \
  -- version
```
This returns the `(major, minor, patch)` version tuple from the `Symbol("ver")` storage key (falling back to `(1, 0, 0)` if the instance was initialized prior to version tracking).

### 2. Execute Upgrade & Migrate
After the new WASM code is deployed:
1. The admin upgrades the WASM via the `upgrade` target.
2. The admin bumps the on-chain version schema by executing the `migrate` function:
   ```bash
   stellar contract invoke \
     --id $CONTRACT_ID \
     --source-account admin-identity \
     --network testnet \
     --send=yes \
     -- migrate --new_version "(1, 1, 0)"
   ```
   *(Note: The `new_version` tuple must be strictly greater than the current version.)*

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
5. Wire production monitors to the probe sequence in
   [CONTRACT_HEALTH.md](CONTRACT_HEALTH.md) (initialized?, admin set?, stats
   sane?, optional Horizon lag)

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
