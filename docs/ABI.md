# Contract ABI Reference

Complete interface reference for **trustbridge-contract**.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [DEPLOYMENT](DEPLOYMENT.md)

---

## Types

### ContributorRecord

```rust
struct ContributorRecord {
    stellar_address: Address,
    registered_at: u64,
    verified: bool,
}
```

### Stats

```rust
struct Stats {
    total: u32,
    verified: u32,
}
```

### Role (u32 discriminant)

```rust
enum Role {
    Admin = 1,
    Upgrader = 2,
    Verifier = 3,
}
```

### ContractError (u32 discriminant)

| Code | Name | Description |
|------|------|-------------|
| 1 | `AlreadyInitialized` | Contract already has an admin |
| 2 | `NotInitialized` | Contract not yet initialized |
| 3 | `NotAuthorized` | Caller lacks permission |
| 4 | `NotRegistered` | Username not in registry |
| 5 | `AlreadyVerified` | Username already verified |
| 6 | `NotVerified` | Cannot revoke verification because the username is not verified |
| 7 | `Paused` | Contract is paused for maintenance or emergency |
| 8 | `CooldownActive` | Upgrade cooldown period has not elapsed |
| 9 | `InvalidVersion` | Target version is not higher than current version |
| 10 | `InvalidRole` | Invalid or unauthorized role assignment |

---

## Functions

### `initialize(admin: Address) -> Result<(), ContractError>`

One-time setup. Stores the admin address and zeroes counters.

| | |
|---|---|
| **Auth** | None (protect at deployment time) |
| **Mutates** | Yes |
| **Errors** | `AlreadyInitialized` |

```bash
stellar contract invoke --id $ID --source deployer --network testnet --send=yes \
  -- initialize --admin G...
```

---

### `register(github_username: String, stellar_address: Address) -> Result<(), ContractError>`

Register or update a GitHub username mapping.

| | |
|---|---|
| **Auth** | `stellar_address` must sign |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `InvalidUsername` |
| **Events** | `RegisteredEvent` |

Behavior:

- New username → increment `count`, append to `idx`
- Existing username → update record; reset `verified` if address changed
- Cold-start registration from an initialized empty registry must expose the
  new record through both `get_address` and admin `get_all_registered`; this is
  covered by the Wave #50 regression test.
- If a verified username is updated to a new Stellar address, verification is
  cleared until the admin verifies the updated address. The Wave #49 regression
  test covers re-verification against the new address.

**Username rules** (enforced on-chain, checked before auth):

| Rule | Value |
|------|-------|
| Length | 1 to 39 characters |
| Allowed characters | `a-z`, `A-Z`, `0-9`, `-`, `_` |
| First and last character | Must be alphanumeric |

Anything else fails with `InvalidUsername` before any signature is verified and
before any storage write, so a rejected call leaves counters and the export
index untouched. Underscores are accepted even though GitHub itself disallows
them, so registrations made before validation existed stay readable and
removable.

```bash
stellar contract invoke --id $ID --source deployer --network testnet --send=yes \
  -- register --github-username octocat --stellar-address G...
```

---

### `get_address(github_username: String) -> Option<ContributorRecord>`

Read-only lookup. Returns `null`/`None` if not registered.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- get_address --github-username octocat
```

---

### `remove(caller: Address, github_username: String) -> Result<(), ContractError>`

Remove a registration.

| | |
|---|---|
| **Auth** | `caller` must sign; must be admin or registrant |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `NotRegistered`, `NotAuthorized` |
| **Events** | `RemovedEvent` |

```bash
# Self-removal (registrant signs)
stellar contract invoke --id $ID --source registrant --network testnet --send=yes \
  -- remove --caller G... --github-username octocat

# Admin removal
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- remove --caller G... --github-username octocat
```

Stats invariant: partial removal decrements `total` only for the removed record
and decrements `verified` only when that removed record was verified. Removing
an unverified record while another verified record remains must leave
`verified` unchanged; this is covered by the Wave #46 regression test.

---

### `get_all_registered() -> Result<Vec<(String, Address)>, ContractError>`

Export the full registry. Admin-only.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | No |
| **Errors** | `NotInitialized` |

```bash
stellar contract invoke --id $ID --source admin --network testnet \
  -- get_all_registered
```

---

### `verify(github_username: String) -> Result<(), ContractError>`

Mark a contributor as verified after off-chain GitHub identity confirmation.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `NotRegistered`, `AlreadyVerified` |
| **Events** | `VerifiedEvent` |

```bash
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- verify --github-username octocat
```

---

### `revoke_verification(github_username: String) -> Result<(), ContractError>`

Revoke verification for a registered contributor. Admin-only.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `NotRegistered`, `NotVerified` |
| **Events** | `VerificationRevokedEvent` |

```bash
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- revoke_verification --github-username octocat
```

---

### `get_verified_count() -> u32`

Returns the number of verified registrations.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- get_verified_count
```

---

### `get_stats() -> Stats`

Returns `{ total, verified }` registration counts.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- get_stats
```

---

### `pause() -> Result<(), ContractError>`

Pauses all state-mutating contract operations. Admin-only.

---

### `unpause() -> Result<(), ContractError>`

Unpauses state-mutating contract operations. Admin-only.

---

### `is_paused() -> bool`

Returns true if contract mutations are currently paused.

---

### `set_role(target: Address, role: Role) -> Result<(), ContractError>`

Assigns an administrative or operational role (`Admin`, `Upgrader`, `Verifier`). Admin-only.

---

### `remove_role(target: Address) -> Result<(), ContractError>`

Revokes a role from an address. Admin-only.

---

### `get_role(address: Address) -> Option<Role>`

Queries assigned role for an address.

---

### `set_cooldown(cooldown_seconds: u64) -> Result<(), ContractError>`

Configures the WASM upgrade timelock cooldown period in seconds. Admin-only.

---

### `get_cooldown() -> u64`

Returns the current WASM upgrade timelock cooldown period in seconds.

---

### `get_version() -> (u32, u32, u32)`

Returns contract version tuple `(major, minor, patch)`.

---

### `upgrade(new_wasm_hash: BytesN<32>) -> Result<(), ContractError>`

Upgrades the executable WASM bytecode of the contract. Subject to admin authentication and upgrade timelock cooldown.

---

### `migrate(new_version: (u32, u32, u32)) -> Result<(), ContractError>`

Updates the contract schema version following a WASM upgrade. Target version must be strictly higher than current version. Admin-only.

---

## Events

All events are defined with `#[contractevent]` and include a topic field for filtering.

### RegisteredEvent

```
topics: ["RegisteredEvent", github_username]
data:   { stellar_address, timestamp }
```

### RemovedEvent

```
topics: ["RemovedEvent", github_username]
data:   { stellar_address, timestamp }
```

### VerifiedEvent

```
topics: ["VerifiedEvent", github_username]
data:   { stellar_address, timestamp }
```

### VerificationRevokedEvent

```
topics: ["VerificationRevokedEvent", github_username]
data:   { stellar_address, timestamp }
```

### UpgradedEvent

```
topics: ["UpgradedEvent", new_wasm_hash]
data:   { version, timestamp }
```

### PausedEvent / UnpausedEvent

```
topics: ["PausedEvent" / "UnpausedEvent", admin]
data:   { timestamp }
```

### RoleGrantedEvent / RoleRevokedEvent

```
topics: ["RoleGrantedEvent" / "RoleRevokedEvent", address]
data:   { role, admin, timestamp }
```

---

### `version() -> (u32, u32, u32)`

Returns the deployed contract version as `(major, minor, patch)`.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

The version is written to instance storage by `initialize`. Instances deployed
before version tracking existed carry no stored version and report the build
constant `1.0.0` instead.

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- version
```

---

### `is_compatible(major: u32, minor: u32, patch: u32) -> bool`

Reports whether the deployed contract satisfies a client's minimum required
version.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

Rules:

- A higher major version is always compatible with a lower required major
- Within the same major, the deployed minor and patch must be at least the
  required ones
- A lower deployed version than required returns `false`

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- is_compatible --major 1 --minor 0 --patch 0
```

---

## TypeScript Bindings

The Stellar CLI generates a typed client package straight from the deployed
WASM, so the bindings never drift from the contract that produced them.

```bash
make bindings CONTRACT_ID=$CONTRACT_ID NETWORK=testnet
make bindings-build CONTRACT_ID=$CONTRACT_ID NETWORK=testnet   # also installs and compiles
```

| Variable | Default | Purpose |
|----------|---------|---------|
| `CONTRACT_ID` | *(required)* | Deployed contract to read the ABI from |
| `NETWORK` | `testnet` | Network the contract lives on |
| `BINDINGS_DIR` | `bindings/typescript` | Output directory for the generated package |
| `PKG_MANAGER` | `pnpm` | Package manager used by `bindings-build` |

The output directory is git-ignored. Generated bindings are a build artifact,
not source: regenerate them after every deploy rather than committing them.

### Version handshake

Call `is_compatible` once at client startup and fail fast when the deployed
contract is older than the bindings expect:

```ts
const client = new Client({ contractId, networkPassphrase, rpcUrl });

const { result: compatible } = await client.is_compatible({
  major: 1,
  minor: 0,
  patch: 0,
});

if (!compatible) {
  const { result: deployed } = await client.version();
  throw new Error(
    `trustbridge-contract ${deployed.join(".")} is older than this client requires`,
  );
}
```

Read-only calls simulate against RPC and never submit a transaction, so the
handshake costs no fees.

### Regeneration checklist

1. Bump `CONTRACT_VERSION` in `src/lib.rs` for any ABI change.
2. Deploy, then run `make bindings CONTRACT_ID=...`.
3. Bump the minimum version in the consuming client's handshake.

A contract change that alters the ABI without a version bump leaves clients
unable to detect the drift, so the bump is a review blocker.

---

## Cost and Benchmarks

Every state-changing call consumes ledger CPU instructions and memory. The
benchmark suite lives with the unit tests in `src/lib.rs` under the
`// === Cost benchmarks` section and reports metered cost per operation using
`env.cost_estimate().budget()`.

```bash
make bench            # print CPU/memory cost for every benchmarked operation
make bench-export     # export-only run, results written to bench-results.txt
```

Output is CSV so it can be diffed between branches:

```
operation,size,cpu_instructions,memory_bytes
get_all_registered,1,...,...
get_all_registered,10,...,...
get_all_registered,50,...,...
get_all_registered,100,...,...
```

### What is benchmarked

| Benchmark | Covers |
|-----------|--------|
| `test_bench_export_cpu_cost` | `get_all_registered` at registry sizes 1, 10, 50, 100 |
| `test_bench_core_operation_cpu_cost` | `register`, `get_address`, `get_stats` |
| `test_bench_failure_path_costs_less_than_success` | Rejected `verify` versus accepted `verify` |

### Regression guards

Absolute instruction counts shift between `soroban-sdk` releases, so the suite
asserts on shape rather than fixed numbers:

- Export cost is **monotonic** in registry size. A drop means the export stopped
  visiting every record.
- Export cost at size 100 stays within **3x the size ratio** of the size 1
  baseline. This passes for a linear scan and fails for quadratic growth.
- A rejected call costs **strictly less** than the equivalent accepted call, so
  a missing-username lookup cannot become a cheap way to burn ledger budget.

### Caveats

- Benchmarks run in the native test host, not in WASM. Numbers are useful for
  comparing branches and spotting complexity regressions, not for predicting
  exact mainnet fees. Use `stellar contract invoke` against testnet for fee
  estimates.
- The measured section resets the budget to unlimited. This keeps cost tracking
  on while removing the ledger ceiling that a 100-entry export would otherwise
  trip mid-measurement.
- `get_all_registered` is admin-only and scans the full index. At large
  contributor counts, prefer event indexing (see
  [EVENT_INDEXING.md](EVENT_INDEXING.md)) over repeated full exports.

---

## CLI Tips

- Use `--` to separate Stellar CLI flags from contract arguments
- Read-only functions simulate locally — no `--send` needed
- State-changing functions require `--send=yes`
- Run `stellar contract invoke --id $ID -- --help` for auto-generated help from the WASM schema

See also: [Stellar CLI invoke argument types](https://developers.stellar.org/docs/tools/cli/cookbook/contract-invoke-arguments)
