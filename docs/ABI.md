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

### ExportPage

Returned by `get_registered_paginated` and `get_public_paginated`. Unlike
`get_all_registered`, each entry is a full `ContributorRecord` — so `verified`
is available directly, with no second call needed (Issue #96).

```rust
struct ExportPage {
    records: Vec<(String, ContributorRecord)>,
    next_cursor: Option<u32>,
    total: u32,
    has_more: bool,
}
```

### BatchSummary

Returned by `batch_verify`. `success_rate` is an integer percentage.

```rust
struct BatchSummary {
    total: u32,
    successful: u32,
    failed: u32,
    success_rate: u32,
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
| 11 | `InvalidUsername` | Username is empty, over `max_username_len`, or contains disallowed characters |

`ContractError::from_code(u32)` maps every code in this table back to the typed
variant and returns `None` for any unrecognized code. All ten codes round-trip
through `from_code(variant.code()) == Some(variant)` — verified by the unit
tests in `src/lib.rs` (`test_from_code_round_trips_all_variants`).

---

## Functions

### `initialize(admin: Address) -> Result<(), ContractError>`

One-time setup. Stores the admin address and zeroes counters.

| | |
|---|---|
| **Auth** | None (protect at deployment time) |
| **Mutates** | Yes |
| **Errors** | `AlreadyInitialized` |

**Admin immutability (Issue #97).** The admin address set here cannot be
overwritten or silently replaced afterward: `initialize` is the only entry
point that writes it, and a second call always fails with
`AlreadyInitialized`, regardless of which admin address it names. No other
function in this ABI mutates the admin. There is no on-chain admin-transfer
API — rotating the admin means deploying a new contract instance. See
[SECURITY.md § Admin Key Management](SECURITY.md#admin-key-management).

```bash
stellar contract invoke --id $ID --source deployer --network testnet --send=yes \
  -- initialize --admin G...
```

---

### `register(github_username: String, stellar_address: Address) -> Result<(), ContractError>`

Register or update a GitHub username mapping.

| | |
|---|---|
| **Auth** | `stellar_address` must sign; if the username is already registered to a *different* address, that address must sign too |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `Paused`, `InvalidUsername` |
| **Events** | `RegisteredEvent` |

**Username validation:**

`github_username` must be a well-formed GitHub handle or the call fails with
`InvalidUsername` (code 11) before any authentication or storage write:

| Rule | Accepted | Rejected |
|---|---|---|
| Length 1–39 characters | `a`, `octocat` | `""`, 40+ characters |
| ASCII only — no Unicode | `user123`, `bob-smith` | `café`, `аlice` (Cyrillic a), `user😀`, `中user` |
| ASCII alphanumerics, `-`, `_` | `user_123`, `bob-smith` | `a@invalid`, `dot.name`, `has space` |
| First and last character alphanumeric | `alice`, `7` | `-invalid`, `invalid-`, `_leading`, `trailing_` |
| No consecutive hyphens | `foo-bar-baz` | `foo--bar` |

Two deliberate choices:

- **Underscores are accepted** even though GitHub itself rejects them. Records
  written before validation existed must stay removable, and `remove` looks a
  username up by exact key — a name that cannot be expressed could never be
  cleaned up.
- **Validation applies to `register` only.** Lookups, `remove`, `verify` and
  `revoke_verification` accept any username, for the same reason.

Checks run *before* `require_auth`, so a malformed username is rejected at the
cheapest point and the caller is not charged for an auth check on an invocation
that can never succeed. It is also what stops an unbounded key from reaching
persistent storage.

Behavior:

- New username → increment `count`, append to `idx`
- Existing username → update record; reset `verified` if address changed
- Existing username pointed at a new address → the **currently registered
  address must also authorize the call**. Without its signature the invocation
  fails at auth, so a username cannot be taken over by whoever calls `register`
  next.
- Cold-start registration from an initialized empty registry must expose the
  new record through both `get_address` and admin `get_all_registered`; this is
  covered by the Wave #50 regression test.
- If a verified username is updated to a new Stellar address, verification is
  cleared until the admin verifies the updated address. The Wave #49 regression
  test covers re-verification against the new address.
- Index-length invariant: every successful `register` increments `COUNT_KEY`
  **and** appends to `INDEX_KEY` atomically. A re-registration of an existing
  username does neither. See [SECURITY.md](SECURITY.md#index-length-invariant).

**Username rules** (enforced on-chain, checked before auth):

| Rule | Value |
|------|-------|
| Length | 1 to 39 characters (read `max_username_len` rather than hardcoding 39) |
| Allowed characters | `a-z`, `A-Z`, `0-9`, `-`, `_` (ASCII only) |
| First and last character | Must be alphanumeric |
| Consecutive hyphens | Not allowed (`foo--bar` → `InvalidUsername`) |
| Non-ASCII / Unicode | Rejected — see [Unicode Rejection Policy](SECURITY.md#unicode-rejection-policy) |

Anything else fails with `InvalidUsername` before any signature is verified and
before any storage write, so a rejected call leaves counters and the export
index untouched. Underscores are accepted even though GitHub itself disallows
them, so registrations made before validation existed stay readable and
removable.

```bash
stellar contract invoke --id $ID --source deployer --network testnet --send=yes \
  -- register --github-username octocat --stellar-address G...
```

**Multi-user register sequence (Issue #94).** For a reference fixture of
`register` called for several users in sequence — expected `RegisteredEvent`
order and `get_stats()` progression after each step — see
`test_issue_94_multi_user_register_sequence` in `src/lib.rs` and
[DASHBOARD_SYNC.md § Multi-user register sequence](DASHBOARD_SYNC.md#multi-user-register-sequence-issue-94).

---

### `max_username_len() -> u32`

Returns the maximum accepted username length (currently `39`). Clients should
read this instead of hardcoding the limit, so relaxing the guard does not
require a client release.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

---

### `is_username_valid(github_username: String) -> bool`

Reports whether a username would pass the `register` guard, so a dashboard can
validate input before asking the user to sign.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

---

### `usernames_match(a: String, b: String) -> bool`

Case-insensitive username equality, matching GitHub's own semantics. Off-chain
verification workflows use this to match a registration against a GitHub
identity without depending on the stored casing.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

Cost is linear in the number of comparisons and allocation-free — the
comparison runs on a fixed stack buffer. `make bench-username` records the
metered CPU/memory cost across 10–200 comparisons.

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
| **Auth** | `caller` must sign; must be admin or registrant — see **Auth model** below |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `Paused`, `NotRegistered`, `NotAuthorized` |
| **Events** | `RemovedEvent` |

```bash
# Self-removal (registrant signs)
stellar contract invoke --id $ID --source registrant --network testnet --send=yes \
  -- remove --caller G... --github-username octocat

# Admin removal
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- remove --caller G... --github-username octocat
```

**Auth model** (Issue #74 / Wave #75):

The authorization check is isolated in `TrustBridgeContract::require_remove_auth`
so it can be read, tested, and changed independently of the mutation logic:

```
caller == admin                    → allowed
caller == record.stellar_address   → allowed (registrant self-removal)
anything else                      → NotAuthorized
```

`caller.require_auth()` runs first — the host verifies the signature before the
policy check. `NotRegistered` is returned before auth when the username does not
exist in storage, so a caller cannot probe username existence by observing whether
they get `NotRegistered` or `NotAuthorized`.

**Error precedence:**

| Condition | Error |
|-----------|-------|
| Contract not initialized | `NotInitialized` |
| Contract paused | `Paused` |
| Username not registered | `NotRegistered` |
| Caller is not admin or registrant | `NotAuthorized` |

**Test coverage** (`src/lib.rs`, Issue #74 / Wave #75):

| Test | Path |
|------|------|
| `test_registrant_can_remove_own_record` | Success — registrant |
| `test_admin_can_remove_any_record` | Success — admin |
| `test_admin_can_remove_record_registered_by_another_user` | Success — admin removes different user's record |
| `test_third_party_cannot_remove` | Failure — `NotAuthorized`, record and count unchanged |
| `test_unknown_address_cannot_remove` | Failure — `NotAuthorized` for fresh address with no role |
| `test_remove_unregistered_username_fails` | Failure — `NotRegistered` |
| `test_remove_already_removed_username_fails` | Failure — `NotRegistered` on double removal |
| `test_remove_blocked_while_paused` | Failure — `Paused` |
| `test_remove_unverified_record_does_not_decrement_verified_count` | Invariant — verified count unchanged |
| `test_remove_verified_record_decrements_verified_count` | Invariant — verified count decremented |
| `test_readding_removed_user_increments_count` | Invariant — re-add starts fresh and unverified |

Stats invariant: partial removal decrements `total` only for the removed record
and decrements `verified` only when that removed record was verified. Removing
an unverified record while another verified record remains must leave
`verified` unchanged; this is covered by the Wave #46 regression test and
`test_remove_unverified_record_does_not_decrement_verified_count`.

Index-length invariant: after every `remove`, `get_stats().total` must equal
the length of the flat username index. Both are updated atomically in the same
transaction. See the **Index-Length Invariant** section in
[SECURITY.md](SECURITY.md#index-length-invariant) and the test suite in
`tests/integration.rs` (Issue #59 / Wave #60).

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

**No `verified` flag.** Entries are `(String, Address)` pairs only — there is
no per-user verification status here. Consumers that need username, address,
and verified together should use `get_registered_paginated` (admin) or
`get_public_paginated` (public) instead, both documented below (Issue #96).

---

### `get_registered_page(offset: u32, limit: u32) -> Result<Vec<(String, Address)>, ContractError>`

Admin-gated page of `(github_username, stellar_address)` pairs, for large
registries where `get_all_registered` would exceed the per-invocation
footprint limit. Same shape as `get_all_registered` — no `verified` flag; use
`get_registered_paginated` for that.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | No |
| **Errors** | `NotInitialized` |

---

### `get_registered_paginated(cursor: u32, limit: u32) -> Result<ExportPage, ContractError>`

Admin-gated paginated export. Each entry is a full `ContributorRecord`, so the
`verified` bit travels with every row — no second call or cross-reference
against `get_address` is needed to know verification status (Issue #96).

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | No |
| **Errors** | `NotInitialized` |

`limit = 0` defaults to `DEFAULT_PAGE_LIMIT` (20); any value above
`MAX_PAGE_LIMIT` (100) is clamped down to it. `next_cursor` is `Some` while
`has_more` is true; pass it back as `cursor` to continue.

```bash
stellar contract invoke --id $ID --source admin --network testnet \
  -- get_registered_paginated --cursor 0 --limit 50
```

---

### `get_public_paginated(cursor: u32, limit: u32) -> Result<ExportPage, ContractError>`

Same `ExportPage` shape as `get_registered_paginated` — including the
`verified` flag per record — but callable by anyone. This is the
unauthenticated read path for dashboards and indexers that need
username + address + verified without an admin key (Issue #96).

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |
| **Errors** | `NotInitialized`, `Paused` |

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- get_public_paginated --cursor 0 --limit 50
```

---

### `verify(github_username: String) -> Result<(), ContractError>`

Mark a contributor as verified after off-chain GitHub identity confirmation.

| | |
|---|---|
| **Auth** | Admin **or** any address assigned `Role::Verifier` (Issue #12) |
| **Caller arg** | `caller: Address` — must be the admin or a `Verifier`-role holder |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `NotRegistered`, `AlreadyVerified`, `NotAuthorized` |
| **Events** | `VerifiedEvent` |

The `caller` argument is required so the contract can validate which identity
signed the transaction. Both the admin and any address granted `Role::Verifier`
via `set_role` may call this function. An address without either role returns
`NotAuthorized`.

```bash
# Admin calling verify
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- verify --caller G... --github-username octocat

# Verifier-role holder calling verify
stellar contract invoke --id $ID --source verifier --network testnet --send=yes \
  -- verify --caller G... --github-username octocat
```

---

### `batch_verify(usernames: Vec<String>) -> Result<BatchSummary, ContractError>`

Verify many contributors in a single invocation — the batched form of `verify`,
for the dashboard-sync workflow where an off-chain job confirms a page of GitHub
identities at once. Doing that as N separate invocations costs N transactions,
N signatures and N rounds of ledger overhead; this is one.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `Paused`, `InvalidBatchSize` |
| **Events** | One `VerifiedEvent` per newly verified contributor |
| **Since** | 1.1.0 — gate on `Version::supports_batch_verify` |

**Partial success is the point.** A username that cannot be verified does not
abort the batch; it is counted as a failure and the rest proceed. A sync of 100
contributors must not be lost wholesale because one entry was removed or already
verified since the off-chain job built its list.

| Outcome | Counted as | Notes |
|---|---|---|
| Registered and unverified | `successful` | Record updated, `VerifiedEvent` published |
| Not registered | `failed` | Skipped, batch continues |
| Already verified | `failed` | Skipped — idempotent, so re-runs are safe |

Inspect the returned `BatchSummary`: a `success_rate` below 100 means some
entries need attention, **not** that the batch failed. The errors listed above
are the only conditions that abort the whole call, and all of them invalidate
every entry rather than a single one.

`verified` is incremented once for the whole batch rather than per entry —
nothing between the per-entry writes can observe an intermediate value within a
single invocation.

```bash
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- batch_verify --usernames '["octocat","alice","bob-smith"]'
```

---

### `revoke_verification(github_username: String) -> Result<(), ContractError>`

Revoke verification for a registered contributor.

| | |
|---|---|
| **Auth** | Admin **or** any address assigned `Role::Verifier` (Issue #12) |
| **Caller arg** | `caller: Address` — must be the admin or a `Verifier`-role holder |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `NotRegistered`, `NotVerified`, `NotAuthorized` |
| **Events** | `VerificationRevokedEvent` |

Like `verify`, the `caller` argument enables on-chain role enforcement. Only
the contract admin or a `Verifier`-role holder may revoke verification. An
`Upgrader`-role holder or an address with no role returns `NotAuthorized`.

```bash
# Admin revoking verification
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- revoke_verification --caller G... --github-username octocat

# Verifier-role holder revoking verification
stellar contract invoke --id $ID --source verifier --network testnet --send=yes \
  -- revoke_verification --caller G... --github-username octocat
```

**Verify → revoke → verify cycle (Issue #95).** `verify` and
`revoke_verification` can be applied to the same username repeatedly without
corrupting counters or storage: revoking decrements `verified`, and a
subsequent `verify` succeeds and publishes a fresh `VerifiedEvent`. Only the
`verified` flag and the `verified` counter change across the cycle — the
record's `stellar_address` and `registered_at` are untouched. A `verify` call
with no intervening `revoke_verification` still fails `AlreadyVerified`, even
after the record has already been through one full cycle. Covered by
`test_issue_95_verify_revoke_verify_cycle` and
`test_issue_95_double_verify_without_revoke_fails_mid_cycle` in `src/lib.rs`.

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

Upgrades the executable WASM bytecode of the contract. Subject to admin
authentication and the upgrade timelock cooldown.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `Paused`, `CooldownActive`, `UnattestedWasm`, `AttestationExpired` |
| **Events** | `UpgradedEvent` |

Records a `WasmProvenance` entry for the new hash: what it replaced, who
authorised it, when, at what version, and whether it had been attested. The
record is written *before* the executable is swapped — afterwards the code
answering the question is the new binary, and what it replaced would be lost.

---

### `attest_upgrade(wasm_hash: BytesN<32>, expires_at: u64) -> Result<(), ContractError>`

Declare in advance the WASM hash you intend to deploy. Admin-only.

| | |
|---|---|
| **Auth** | Admin |
| **Mutates** | Yes |
| **Errors** | `NotInitialized`, `AttestationExpired` (if `expires_at` is not in the future) |

**Optional two-step upgrade.** While an attestation is live, `upgrade` accepts
only the hash it names — so a compromised admin key cannot swap in a different
binary at the moment of the upgrade without first publishing that intent
on-chain, ahead of time, where watchers can see it.

| Situation | `upgrade` behaviour |
|---|---|
| No attestation published | Proceeds as before — attestation is opt-in |
| Attestation matches, unexpired | Proceeds; attestation is consumed; `attested: true` in provenance |
| Attestation expired | Fails `AttestationExpired`; the stale record is cleared so a retry is not blocked |
| Hash does not match | Fails `UnattestedWasm`; the attestation is **left in place**, since a mismatch may be an attacker substituting a binary and clearing it would let a second attempt through unchecked |

Attestations are **single-use** — one upgrade, not a standing permission for
that hash. `expires_at` is mandatory for the same reason: an attestation that
never lapsed would be a standing authorisation, which is worse than none.

Publishing a new attestation replaces any existing one.

```bash
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- attest_upgrade --wasm-hash <hex> --expires-at 1893456000
```

---

### `clear_attestation() -> Result<(), ContractError>`

Withdraw a pending attestation. Admin-only. The escape hatch for one published
in error — without it the admin would have to wait out the expiry before
upgrading to any other hash.

---

### `get_attestation() -> Option<WasmAttestation>`

Returns the pending attestation, if any. Returned regardless of expiry, since
seeing a lapsed attestation is what explains a rejected upgrade.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

---

### `get_provenance() -> Option<WasmProvenance>`

Returns the provenance of the currently deployed WASM. `None` on an instance
that has never been upgraded.

| | |
|---|---|
| **Auth** | None |
| **Mutates** | No |

```bash
stellar contract invoke --id $ID --source deployer --network testnet \
  -- get_provenance
```

---

### `migrate(new_version: (u32, u32, u32)) -> Result<(), ContractError>`

Updates the contract schema version following a WASM upgrade. Target version must be strictly higher than current version. Admin-only.

---

## Events

All events are defined with `#[contractevent]` and include a topic field for filtering.

### RegisteredEvent

```
topics: ["registered_event", github_username]
data:   { stellar_address, timestamp }
```

### RemovedEvent

```
topics: ["removed_event", github_username]
data:   { stellar_address, timestamp }
```

`RemovedEvent` is the only signal an indexer receives that a record is gone, so
its payload is treated as a compatibility surface:

- `github_username` is a **topic**, so subscribers can filter server-side.
- `stellar_address` is the address that was registered at removal time, and
  `timestamp` is the ledger timestamp of the removal — together they let a
  consumer reconstruct the retired record without a follow-up read.
- A **failed** `remove` (wrong caller, unknown username) publishes no event.

`test_removed_event_payload_is_complete` asserts the full published event
against a fully-specified `RemovedEvent`, plus the topic count and topic symbol
independently, so renaming the event or dropping a field fails the build rather
than silently breaking every subscriber's filter.
`test_removed_event_not_published_on_failed_remove` covers the failure path.

### VerifiedEvent

```
topics: ["verified_event", github_username]
data:   { stellar_address, timestamp }
```

`VerifiedEvent` is the primary signal an indexer uses to learn that a
contributor's GitHub identity has been confirmed. Its payload is a
compatibility surface: any rename of the topic symbol, reorder of topics,
or change to the data fields silently breaks every downstream subscriber.

The following tests in `src/lib.rs` pin the full payload (Issue #64 / Wave #65):

| Test | What it checks |
|------|----------------|
| `test_verified_event_payload_is_complete` | Full event list matches: topic symbol `"verified_event"`, `github_username` topic, `stellar_address` and `timestamp` in data |
| `test_verified_event_not_published_on_already_verified` | **Failure path**: `AlreadyVerified` must not publish an event |
| `test_verified_event_not_published_on_unregistered_username` | **Failure path**: `NotRegistered` must not publish an event |
| `test_verified_event_carries_current_stellar_address` | Event carries the address current at verify time, not a stale pre-update address |

### VerificationRevokedEvent

```
topics: ["verification_revoked_event", github_username]
data:   { stellar_address, timestamp }
```

Mirrors `VerifiedEvent`. Indexers subscribe to both and reconcile verified
state from the event stream, so `VerificationRevokedEvent` is the same
compatibility surface.

| Test | What it checks |
|------|----------------|
| `test_verification_revoked_event_payload_is_complete` | Full event list matches: topic symbol, `github_username` topic, `stellar_address` and `timestamp` in data |
| `test_verification_revoked_event_not_published_on_not_verified` | **Failure path**: `NotVerified` must not publish an event |

### UpgradedEvent

```
topics: ["upgraded_event", new_wasm_hash]
data:   { version, timestamp }
```

### PausedEvent / UnpausedEvent

```
topics: ["paused_event" / "unpaused_event", admin]
data:   { timestamp }
```

### RoleGrantedEvent / RoleRevokedEvent

```
topics: ["role_granted_event" / "role_revoked_event", address]
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
| `test_bench_export_cpu_cost` | `get_all_registered` at registry sizes 10, 20, 40, 80 |
| `test_bench_username_case_normalization` | `usernames_match` at 10, 50, 100, 200 comparisons (`make bench-username`) |
| `test_bench_core_operation_cpu_cost` | `register`, `get_address`, `get_stats` |
| `test_bench_failure_path_costs_less_than_success` | Rejected `verify` versus accepted `verify` |

### Regression guards

Absolute instruction counts shift between `soroban-sdk` releases, so the suite
asserts on shape rather than fixed numbers:

- Export cost is **monotonic** in registry size. A drop means the export stopped
  visiting every record.
- Export cost at the largest size stays within **3x the size ratio** of the
  smallest-size baseline. This passes for a linear scan and fails for quadratic
  growth.
- Username case normalization is **monotonic** in comparison count and obeys the
  same 3x linearity ceiling. Normalization runs on a fixed stack buffer, so a
  regression that introduces per-comparison allocation or a nested scan fails
  here.
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
- `get_all_registered` reads one ledger entry per record, and Soroban rejects an
  invocation whose footprint exceeds **100 ledger entries**. The export
  benchmark therefore tops out below that ceiling; past roughly 100
  contributors, `get_registered_page` / `get_registered_paginated` is the only
  workable export path.
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
