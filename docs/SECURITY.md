# Security

Security considerations for **trustbridge-contract**.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [DEPLOYMENT](DEPLOYMENT.md)

---

## Threat Model

### In Scope

| Threat | Mitigation |
|--------|------------|
| Impersonation (registering someone else's GitHub username) | `stellar_address.require_auth()` — only the address owner can register |
| Unauthorized removal | `caller` must auth as registrant or admin |
| Unauthorized admin actions | `admin.require_auth()` on `verify`, `revoke_verification`, `get_all_registered`, and all admin-only functions |
| Double initialization | `AlreadyInitialized` error |
| Admin storage mutated after init | No public setter writes `ADMIN_KEY` — only `initialize` does, gated by `AlreadyInitialized` (Issue #97) |
| Malformed or oversized username input | `InvalidUsername` error, checked before auth and before any write |
| Unicode / homoglyph username spoofing | Byte-wise ASCII validation rejects all non-ASCII bytes; see **Unicode Rejection Policy** section |
| Consecutive-hyphen username bypass | `InvalidUsername` error — consecutive hyphens now enforced on-chain |
| Counter drift from rejected calls | Invariant property fuzzing, see [REGISTRY_INVARIANTS](REGISTRY_INVARIANTS.md) |
| Stale trust surviving a remove → re-register cycle (a new registrant inheriting the previous owner's verified status or address binding) | `remove` unconditionally clears the stored record; `register` on a removed username always starts a fresh, unverified record — see [Re-registration After Remove](#re-registration-after-remove) (Issue #93) |
| Compromised or unpinned RPC client dependency | Crate validation checklist below |

### Out of Scope (handled off-chain)

| Concern | Responsibility |
|---------|----------------|
| GitHub identity proof | Admin verification workflow + TrustBridge dashboard |
| Username squatting policy | Social/process layer; contract allows first-come registration |
| Admin key compromise | Operational security; use multisig for admin address |
| GitHub username changes | Off-chain mapping updates; may require re-registration |

---

## Admin Key Management

The admin address is **immutable** after `initialize` (Issue #97). `initialize`
is the only entry point that writes the `ADMIN_KEY` storage slot, and it does
so exactly once: a second call fails with `AlreadyInitialized` regardless of
which admin address it names. No other public function in the ABI — `pause`,
`set_role`, `set_cooldown`, `migrate`, `upgrade`, etc. — mutates `ADMIN_KEY`.
There is deliberately no admin-transfer API (see Notes on Issue #97); rotation
means redeploying a new instance.

Regression coverage in `src/lib.rs`:

- `test_double_initialize_rejected_after_successful_init`,
  `test_issue_97_second_initialize_rejected_with_different_admin` — a second
  `initialize` always fails, even with a different admin address.
- `test_issue_97_admin_unchanged_across_unrelated_operations` — the original
  admin remains the only recognized admin across pause/unpause, role grants,
  cooldown changes, verify, and migration.

Recommendations:

- Use a **multisig** or **smart account** as the admin G-address
- Never commit private keys or seed phrases
- Rotate operational keys via deploying a new contract instance if admin is compromised (no on-chain admin transfer in v0.1)

---

## Registration Integrity

- Registering a username requires the Stellar address owner to sign
- Re-registration with a new address resets verification status
- There is no on-chain proof of GitHub ownership at registration time — verification is a separate admin step
- Wave #49 locks the address-update invariant: after a verified username is
  re-registered to a different Stellar address, the record becomes unverified,
  the verified count decreases, and any later `verify()` applies to the new
  address only.

---

## Username Squatting Mitigations

Because Soroban handles GitHub registrations permissionlessly (first-come, first-registered), there is a risk of username squatting (someone registering another contributor's GitHub username to redirect their rewards). TrustBridge uses a multi-layered security model to mitigate this risk.

### 1. Mandatory Admin Verification Gate
Registration alone does **not** grant payout readiness. Payout systems and the TrustBridge dashboard require a contributor record to be **verified** before rewards can be disbursed.
- Verification is performed by the contract admin or a designated verifier after confirming ownership of the GitHub account off-chain (e.g., via OAuth or a cryptographic proof).
- The verifier validates that the registered Stellar address matches the authenticated GitHub user.
- If a squatter registers a name, they cannot pass this verification gate since they cannot prove ownership of the corresponding GitHub account.

### 2. Double-Auth Transfer Protection (Self-Auth)
If a user registers a username and later needs to transfer it to a different Stellar address, the contract requires **both** of the following to authorize the transaction:
1. The new Stellar address.
2. The currently registered Stellar address.
This prevents a third party from maliciously taking over a registered username.

### 3. Contributor Dispute & Resolution Flow
If a rightful owner discovers that their GitHub username has been squatted on-chain:
1. **Report**: The owner reports the dispute to the TrustBridge administrators (off-chain).
2. **Revocation/Removal**: The admin verifies the owner's identity, then calls `remove` to delete the squatter's record from the contract registry.
3. **Re-registration**: The rightful owner registers their correct Stellar address.
4. **Re-verification**: The admin verifies the new record.

### FAQ: "Someone registered my GitHub name, what should I do?"
- **Will they receive my payouts?** No. Payouts require the record to be verified. The squatter cannot pass the admin verification check.
- **How do I reclaim my username?** Open a support ticket / dispute with the TrustBridge administrators. They will remove the squatter's record so you can register your address.
- **Does the contract verify my GitHub handle automatically?** No. There is **no on-chain verification proof** of GitHub identity at registration time. Verification is entirely off-chain/administrative.

---

## Input Validation

`register` validates the username **before** `require_auth()` and before any
storage write. The order matters: a malformed call is rejected at the cheapest
point, no signature is spent on it, and no counter or index entry moves.

| Rule | Value |
|------|-------|
| Length | 1 to 39 characters (GitHub's own cap) |
| Allowed characters | `a-z`, `A-Z`, `0-9`, `-`, `_` (ASCII only) |
| First and last character | Must be alphanumeric |
| Consecutive hyphens | Not allowed (`foo--bar` is rejected) |
| Unicode / non-ASCII | Rejected — see **Unicode Rejection Policy** below |

Rejection returns `InvalidUsername` (code 7).

Validation lives in `src/utils.rs` and works entirely on a fixed 64-byte stack
buffer. The contract is `#![no_std]`, so the validation path never allocates
and the copy length is bounded before the copy happens.

Deliberate non-goals:

- **Underscores are accepted** even though GitHub disallows them, so any
  registration made before validation existed stays readable and removable.
  Tightening this later would strand those records.
- **Case is not normalized on-chain.** `Alice` and `alice` are distinct keys.
  Off-chain workflows should match with `eq_ignore_ascii_case` from
  `src/utils.rs` when comparing a registration against a GitHub identity.
- **No on-chain proof the username exists on GitHub.** Validation checks shape,
  not ownership. Ownership remains the admin verification step.

---

## Unicode Rejection Policy

**GitHub usernames are ASCII-only.** Any username containing a non-ASCII byte —
including multi-byte UTF-8 sequences for accented letters (é, ü, ñ), emoji,
CJK characters, or Cyrillic/Arabic/Hebrew script — is rejected with
`InvalidUsername`.

### Why this matters

Unicode homoglyph attacks are a recognized impersonation vector. An attacker
registers a username that **looks** visually identical to a legitimate user's
name but uses different Unicode codepoints:

- Cyrillic 'а' (U+0430) looks like ASCII 'a' (U+0061)
- Greek 'ο' (U+03BF) looks like ASCII 'o' (U+006F)
- Cyrillic 'с' (U+0441) looks like ASCII 'c' (U+0063)

A username like `аlice` (Cyrillic 'а' + ASCII 'lice') appears indistinguishable
from `alice` in most fonts, but encodes as `[0xD0, 0xB0, 0x6C, 0x69, 0x63, 0x65]`
instead of `[0x61, 0x6C, 0x69, 0x63, 0x65]`. Without byte-level validation,
this becomes a credential spoofing attack.

### How the check works

Validation is byte-wise, not glyph-wise:

1. Every username is copied into a fixed stack buffer (64 bytes).
2. Every byte is checked with `.is_ascii()` (returns false for bytes > 0x7F).
3. Any multi-byte UTF-8 sequence has a leading byte ≥ 0x80, which fails the
   ASCII check and is immediately rejected.

This makes the homoglyph attack impossible: even if the rendered glyphs look
identical, the byte sequences differ and only the ASCII form is accepted.

### Covered cases

The following are all rejected (see comprehensive tests in `src/utils.rs`):

| Category | Example | Codepoint | UTF-8 Encoding |
|----------|---------|-----------|----------------|
| Latin-extended | `café` | U+00E9 é | `[0xC3, 0xA9]` |
| Emoji | `user😀` | U+1F600 | `[0xF0, 0x9F, 0x98, 0x80]` |
| CJK (Chinese/Japanese/Korean) | `中user` | U+4E2D 中 | `[0xE4, 0xB8, 0xAD]` |
| Arabic | `مuser` | U+0645 م | `[0xD9, 0x85]` |
| Hebrew | `אuser` | U+05D0 א | `[0xD7, 0x90]` |
| Cyrillic homoglyph | `аlice` | U+0430 а | `[0xD0, 0xB0]` |
| Greek homoglyph | `bοb` | U+03BF ο | `[0xCF, 0xBF]` |

### Test coverage

`src/utils.rs` includes a dedicated test suite for the Unicode rejection policy
(Wave #69 / Issue #70):

- `test_unicode_latin_extended_rejected`
- `test_unicode_emoji_rejected`
- `test_unicode_cjk_rejected`
- `test_unicode_arabic_and_rtl_rejected`
- `test_unicode_homoglyph_attack_rejected`
- `test_unicode_all_non_ascii_rejected`
- `test_unicode_embedded_at_any_position_rejected`
- `test_raw_high_byte_rejected`
- `test_valid_ascii_still_accepted_after_unicode_hardening`

These tests confirm that every form of non-ASCII input — whether a visually
distinct character like an emoji or a deceptive homoglyph like Cyrillic 'а' —
is caught and rejected, while every valid ASCII username shape remains accepted.

### Performance

The check adds no allocations and no UTF-8 decoding overhead. It is a
per-byte scan over a stack buffer, the same cost profile as the existing
alphanumeric and hyphen checks.

### Future considerations

- Off-chain tooling (dashboard, indexers) should **canonicalize and validate**
  usernames against the GitHub API before submitting them for registration.
  The on-chain check is a last line of defense, not a substitute for
  pre-submission validation.
- If GitHub's own username policy changes (e.g. to allow certain Unicode
  ranges), this validation will need to be relaxed via a contract upgrade and
  a corresponding audit of the new attack surface.

---

---

## Validating the Rust RPC Client Crate

Every off-chain component that talks to this contract, including the deploy
scripts, the dashboard sync job, and any indexer, reaches the network through an
RPC client crate. That crate sits between operator keys and the network, so it
is in the trust boundary and gets reviewed like contract code.

### Before adding or bumping an RPC client dependency

| Check | How |
|-------|-----|
| Version is pinned exactly | `soroban-client = "=x.y.z"` in `Cargo.toml`, `Cargo.lock` committed for binaries |
| No known advisories | `cargo audit` and `cargo deny check advisories` |
| License is acceptable | `cargo deny check licenses` |
| No unexpected transitive additions | `cargo tree --duplicates` and review the lockfile diff |
| Source is the official crate | Confirm the repository field points at the upstream Stellar org, not a fork |
| Registry integrity | `cargo verify-project`; do not use `[patch]` or git dependencies for release builds |
| Maintenance signal | Recent releases, open advisories, and responsiveness on upstream issues |

A dependency bump that changes the transitive graph needs the lockfile diff in
the PR. Reviewers should be able to see every crate that was added.

### Runtime expectations for any RPC client

- **TLS enforced.** Reject plain `http://` RPC URLs outside of local development.
- **No secret logging.** Secret keys, seed phrases, and signed transaction
  envelopes must never reach logs, error strings, or telemetry.
- **Bounded retries.** Retry with exponential backoff and a hard attempt cap, so
  an outage degrades instead of turning into a self-inflicted flood.
- **Explicit timeouts.** A client with no timeout turns an RPC stall into a hung
  deploy job holding an operator key in memory.
- **Response validation.** Treat RPC responses as untrusted input: check the
  contract ID, network passphrase, and ledger sequence before acting on them.
- **Simulation before submission.** Simulate state-changing calls first so a
  malformed username or an auth failure surfaces without spending fees.

---

## Operational Failure Modes

| Failure | Expected behavior | Operator action |
|---------|-------------------|-----------------|
| Horizon or RPC outage | Client retries with backoff, then fails loudly. Contract state is unaffected: nothing was submitted. | Fail the job, alert, retry later. Never fall back to an unverified RPC endpoint. |
| RPC rate limiting (HTTP 429) | Backoff honors `Retry-After` where present. | Reduce poll frequency, batch reads, or move to a dedicated RPC provider. |
| Invalid env configuration | `scripts/deploy.sh` refuses to run without `ADMIN`. Every `invoke-*` and `bindings` Makefile target refuses to run without `CONTRACT_ID`, and `invoke-init` also requires `ADMIN`. | Fix the value rather than exporting a placeholder. `NETWORK` defaults to `testnet`, so a mainnet job must state `NETWORK=mainnet` explicitly. |
| Auth or permission failure | `require_auth()` panics the invocation and the whole transaction rolls back. Admin-only calls by a non-admin return `NotAuthorized`. | Confirm the signing key matches the registrant or the admin address. |
| Partial write during failure | Not possible. Soroban transactions are atomic, and validation runs before the first write. | None. |
| 100+ contributor scale | `get_all_registered` is a linear full-index scan and grows with registry size. | Prefer event indexing (see [EVENT_INDEXING.md](EVENT_INDEXING.md)) over repeated full exports. Watch the export benchmark in [ABI.md](ABI.md#cost-and-benchmarks) for regressions. |

### Environment configuration

Copy `.env.example` and fill every value explicitly. Configuration rules:

- No implicit network default in production scripts. `NETWORK` must be stated.
- `ADMIN` is required for mainnet deploys and is not inferred from the local
  keystore.
- Never commit `.env`. Only `.env.example` is tracked.

---

## Index-Length Invariant

The registry maintains two parallel state values that must always agree:

| State | Storage key | Updated by |
|-------|-------------|------------|
| `COUNT_KEY` — registration counter (`u32`) | instance storage | `register` (increment), `remove` (decrement) |
| `INDEX_KEY` — ordered username vec (`Vec<String>`) | instance storage | `add_to_index` (append), `remove_from_index` (filter) |

**Invariant:** `get_count(env) == get_index(env).len()` at every quiescent point between transactions.

### Why this matters

Both values are read by different callers for different purposes:

- Paginated export endpoints (`get_registered_page`, `get_registered_paginated`, `get_public_paginated`) walk `INDEX_KEY` for the actual usernames but expose `COUNT_KEY` as the `total` field of the response. If they diverge, a client that uses `total` to compute page counts will request the wrong number of pages.
- `get_stats` returns `COUNT_KEY` directly. Monitoring and dashboard tooling that reads `get_stats` to show a contributor count will display a wrong number if the counter has drifted.
- An index longer than the counter indicates **phantom entries** — the index holds usernames that the contract believes do not exist. An index shorter than the counter indicates **invisible entries** — the counter says more contributors exist than are reachable by any export. Both are security-relevant for an audit.

### How the invariant is maintained

`register` and `remove` always update both values in the same transaction:

```
register (new username):
    set_count(get_count + 1)
    add_to_index(username)        ← appends to INDEX_KEY

remove:
    remove_record(username)
    remove_from_index(username)   ← filters INDEX_KEY
    set_count(get_count - 1)
```

Soroban transactions are atomic, so a partial write that updates one side but not the other cannot leave the invariant broken at rest — either both updates land or neither does.

### Test coverage (Issue #59 / Wave #60)

`tests/integration.rs` includes a dedicated invariant test suite:

| Test | What it checks |
|------|----------------|
| `test_index_invariant_holds_on_empty_registry` | Invariant holds at genesis (count=0, index.len()=0) |
| `test_index_invariant_holds_after_single_register` | Invariant holds after the first registration |
| `test_index_invariant_holds_after_register_and_remove` | Invariant holds after removing first, middle, and last entries |
| `test_index_invariant_holds_after_same_address_reregister` | Re-register to same address does not double-increment counter |
| `test_index_invariant_holds_after_address_change_reregister` | Re-register to different address does not alter total |
| `test_index_invariant_holds_at_scale` | Register 10, remove 5 interleaved — check after each removal |
| `test_index_invariant_unchanged_on_failed_remove` | **Failure path**: `remove` on unknown username returns `NotRegistered` and does not mutate state |
| `test_index_invariant_unchanged_on_invalid_register` | **Failure path**: invalid username returns `InvalidUsername` and does not mutate state |
| `test_index_invariant_holds_after_remove_then_reregister` | Remove then re-register restores count=1, index.len()=1 |
| `test_index_invariant_unchanged_by_pause_unpause` | Pause/unpause does not touch count or index |

The helper `storage::index_length_invariant_holds(env)` encodes `get_count == get_index().len()` in one place so every test asserts the same invariant without repeating the definition inline.

### Edge cases

- **Removal of a non-existent username** returns `NotRegistered` before any write, so count and index are never touched on a failed remove.
- **Invalid username on register** is caught before `require_auth` and before any write, so count and index are never touched on a rejected registration.
- **Re-registration** (same username, same or different address) follows the `existing.is_some()` branch in `register`, which does not call `add_to_index` or increment the counter, preserving the invariant.
- **100+ contributors**: both `COUNT_KEY` and `INDEX_KEY` live in instance storage. At very large registry sizes the `get_all_registered` export hits the 100-ledger-entry footprint limit; use paginated endpoints instead, but the invariant is unaffected by which export endpoint is used.

---

## Storage TTL

Persistent entries on Stellar mainnet have a **time-to-live (TTL)**. If entries expire, data may become unavailable until extended.

Operational teams should:

1. Monitor entry TTL via RPC
2. Run periodic TTL extension via Stellar CLI (`stellar contract extend`)
3. Document extension cadence in deployment runbooks

---

## Remove Auth Negative Matrix

Auth gaps on `remove` are high-impact — an unauthorized removal erases a contributor's identity mapping without their consent.
The full failure surface is documented here and covered by automated unit tests in `src/lib.rs` (search for `#113`).

Cross-reference: [verify/revoke_verification negative matrix](#verify-and-revoke_verification-auth-negative-matrix) (Issue #114) · [ABI reference](ABI.md#removecaller-address-github_username-string---resultcontracterror)

| # | Scenario | Expected error | Code | Test |
|---|----------|---------------|------|------|
| 1 | Contract not yet initialized | `NotInitialized` | 2 | `test_remove_negative_not_initialized` |
| 2 | Username not registered | `NotRegistered` | 4 | `test_remove_negative_not_registered` |
| 3 | Caller is a random address (not admin, not registrant) | `NotAuthorized` | 3 | `test_remove_negative_wrong_caller_random_address` |
| 4 | **Registrant removes their own record** _(happy path)_ | `Ok(())` | — | `test_remove_positive_registrant_can_remove_own` |
| 5 | **Admin removes any registration** _(happy path)_ | `Ok(())` | — | `test_remove_positive_admin_can_remove_any` |
| 6 | Third-party address with no role | `NotAuthorized` | 3 | `test_remove_negative_third_party_no_role` |
| 7 | `Role::Upgrader` holder (not registrant) | `NotAuthorized` | 3 | `test_remove_negative_upgrader_role_cannot_remove` |
| 8 | `Role::Verifier` holder (not registrant) | `NotAuthorized` | 3 | `test_remove_negative_verifier_role_cannot_remove` |
| 9 | Contract is paused | `Paused` | 7 | `test_remove_negative_paused` |

### Auth rules for `remove`

```
caller == admin   →  allowed
caller == record.stellar_address  →  allowed
otherwise         →  NotAuthorized (code 3)
```

The `caller` address is a required argument because Soroban contracts cannot inspect the
transaction source account without an explicit argument. Passing `caller` allows the contract
to call `caller.require_auth()` and then check the above conditions in a single, auditable step.

No special role (Upgrader, Verifier, or any future custom role) grants remove rights.
Only the two identities above may remove a record — by design, to prevent privilege-escalation
vectors where a broadly-held operational role could silently wipe contributor mappings.

---

## Responsible Disclosure

If you discover a security vulnerability:

1. **Do not** open a public GitHub issue
2. Email the maintainers or use GitHub Security Advisories on the repository
3. Include steps to reproduce, impact assessment, and suggested fix if available

We aim to acknowledge reports within 72 hours.

---

## Futurenet Deploy Smoke Workflow

Wave #39: before an audit or a testnet/mainnet promotion, validate a fresh
deploy against Futurenet to catch threat-model regressions early (e.g. an
`initialize` gate that silently no-ops, or a lookup that leaks state before
verification).

1. Deploy to Futurenet: `ADMIN=G... ./scripts/futurenet_smoke_test.sh`
2. Confirm `get_stats` reports `{total: 0, verified: 0}` on the fresh instance
   — a nonzero result means the deploy reused stale storage.
3. Confirm `has_record` returns `false` for an unregistered username — this
   guards the "no on-chain proof of GitHub ownership" boundary called out
   above by verifying reads don't fabricate positive results.
4. Re-run after any change to `initialize`, `register`, or storage key
   layout, since those are the surfaces the threat model above depends on.

The script is a deploy sanity check, not a substitute for `cargo test`
(see `src/lib.rs` and `tests/integration.rs` for functional coverage).

---

## Verify and Revoke Verification CLI Usage

The `verify` and `revoke_verification` functions are admin-only in the CLI documentation. The authoritative examples below use `--source = admin`. A non-admin caller (including a registrant) receives `NotAuthorized` and the transaction reverts.

### Authoritative examples (admin path)

```bash
# Verify a contributor (admin must sign)
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- verify --caller G... --github-username octocat

# Revoke verification (admin must sign)
stellar contract invoke --id $ID --source admin --network testnet --send=yes \
  -- revoke_verification --caller G... --github-username octocat
```

### Unauthorized failure (non-admin)

If a non-admin address attempts either call, the transaction fails with `NotAuthorized`:

```bash
# This will fail — registrant cannot self-verify
stellar contract invoke --id $ID --source registrant --network testnet --send=yes \
  -- verify --caller G... --github-username octocat
# Error: NotAuthorized (code 3)
```

Do not construct CLI examples that imply a registrant can self-verify. The contract rejects such calls at the auth layer.

---

## Audit Status

This contract has **not** been formally audited. Use at your own risk on mainnet until an audit is completed.

For production deployments, consider:

- Independent security audit
- Bug bounty program
- Staged rollout on testnet/futurenet first
