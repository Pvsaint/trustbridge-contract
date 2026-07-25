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
| **Errors** | `NotInitialized` |
| **Events** | `RegisteredEvent` |

Behavior:

- New username → increment `count`, append to `idx`
- Existing username → update record; reset `verified` if address changed

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

## CLI Tips

- Use `--` to separate Stellar CLI flags from contract arguments
- Read-only functions simulate locally — no `--send` needed
- State-changing functions require `--send=yes`
- Run `stellar contract invoke --id $ID -- --help` for auto-generated help from the WASM schema

See also: [Stellar CLI invoke argument types](https://developers.stellar.org/docs/tools/cli/cookbook/contract-invoke-arguments)
