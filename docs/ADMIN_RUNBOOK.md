# Admin Runbook

The admin role exists for off-chain GitHub verification and operational recovery.

Related docs: [SECURITY](SECURITY.md) · [ABI](ABI.md) · [DEPLOYMENT](DEPLOYMENT.md) · [EVENT_INDEXING](EVENT_INDEXING.md)

## Routine actions

- Verify contributors only after confirming the GitHub identity off-chain.
- Revoke verification cleanly when contributor identities change or a registration is invalidated.
- Export registered records before large dashboard migrations.
- Keep the admin account in a secure wallet or multisig flow.

---

## Emergency Pause Lifecycle

In case of a detected security vulnerability, operational incident, or during maintenance windows, the contract admin can pause all state mutations.

### 1. Trigger Pause
To pause the contract:
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account admin-identity \
  --network testnet \
  --send=yes \
  -- pause
```
This sets the internal `Symbol("pause")` state to `true` and publishes a `PausedEvent`.

### 2. Restore Normal Operations (Unpause)
Once maintenance is complete or the incident is resolved:
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account admin-identity \
  --network testnet \
  --send=yes \
  -- unpause
```
This restores operations and publishes an `UnpausedEvent`.

### 3. Function Behavior During Pause

While the contract is paused, functions behave as follows:

| Gated & Blocked (Panics with `ContractError::Paused`) | Allowed (Read-only or non-mutating) |
|-------------------------------------------------------|--------------------------------------|
| `register`                                            | `is_paused`                          |
| `remove`                                              | `is_contract_paused`                 |
| `verify`                                              | `get_address`                        |
| `batch_verify`                                        | `has_record`                         |
| `revoke_verification`                                 | `get_role`                           |
| `set_role`                                            | `get_cooldown`                       |
| `remove_role`                                         | `get_version`                        |
| `upgrade`                                             | `version`                            |
| `migrate`                                             | `is_compatible`                      |
| `get_public_paginated`                                | `max_username_len`                   |
|                                                       | `is_username_valid`                  |
|                                                       | `usernames_match`                    |
|                                                       | `get_registered_page`                |
|                                                       | `get_all_registered`                 |
|                                                       | `get_registered_paginated`           |
|                                                       | `get_stats`                          |
|                                                       | `get_verified_count`                 |
|                                                       | `get_provenance`                     |
|                                                       | `get_attestation`                    |

*Note: Administrative read operations (`get_all_registered`, `get_registered_page`, `get_registered_paginated`) remain accessible to the admin to facilitate data export during maintenance.*

### 4. Simulation & Validation
You can simulate the entire pause/unpause flow using the provided Makefile target:
```bash
make simulate-pause-flow CONTRACT_ID=$CONTRACT_ID NETWORK=testnet SOURCE=admin-identity
```

### 5. Ops Resource & Performance Notes
- **Metered CPU/Memory Cost**: Initiating a pause/unpause uses minimal Soroban resources (~130,000 CPU instructions and ~10KB RAM) as it only mutates a single instance storage boolean flag and publishes one event.
- **Client Latency**: Client check integrations calling `is_paused` locally via RPC simulate in 0ms and consume no network gas.

---

## Recovery notes

If an admin key is rotated in a future contract version, announce the new admin address and keep the old deployment metadata available for auditors.

---

## Mainnet incident: emergency verification revoke

**Purpose.** Stop trust in a compromised or invalid GitHub → Stellar mapping as
fast as possible without deleting the registration.

**Revoke is the default incident response.** Prefer
`revoke_verification` over `remove` whenever clearing the verified flag is
enough. See [Revoke vs remove](#revoke-vs-remove-warning) below.

Cross-links:

- Auth and threat model: [SECURITY.md](SECURITY.md)
- Exact ABI, errors, and CLI shapes: [ABI.md](ABI.md#revoke_verificationgithub_username-string---result-contracterror)
- Event consumers: [EVENT_INDEXING.md](EVENT_INDEXING.md)

### When to use `revoke_verification`

Use revoke when **any** of the following is true:

| Trigger | Why revoke |
|---------|------------|
| Suspected key compromise of a verified Stellar address | Stops payouts/trust while the username mapping remains inspectable |
| Off-chain GitHub identity check fails after prior verify | Clears `verified` without destroying registry history |
| Contributor reports unauthorized registration still marked verified | Fastest way to withdraw the verification signal |
| Compliance / security requires immediate “do not trust” | Emits `VerificationRevokedEvent` for indexers and auditors |

Do **not** use revoke when the record should leave the registry entirely (for
example a confirmed squatter after legal/process review). That is a separate
`remove` decision, not the first incident action.

### Authorization / authentication

From the contract (`src/lib.rs`) and [ABI.md](ABI.md):

1. Contract must be **initialized** and **not paused**.
2. Transaction must include `caller: Address` and that address must
   `require_auth()`.
3. `caller` must be the contract **admin** **or** hold `Role::Verifier`
   (`has_role_or_admin(..., Role::Verifier)`).
4. `Upgrader` (or any address without admin/Verifier) returns `NotAuthorized`.

Operational auth checklist:

- [ ] Confirm you are signing with the mainnet admin identity **or** a known
      Verifier-role key (prefer multisig admin for mainnet).
- [ ] Confirm `CONTRACT_ID` and `NETWORK=mainnet` (never reuse testnet IDs).
- [ ] Confirm the username spelling (on-chain keys are **case-sensitive**; see
      [SECURITY.md](SECURITY.md#input-validation)).
- [ ] Prefer simulate-then-send: run the invoke **without** `--send=yes` first,
      then re-run with `--send=yes` only after the simulation succeeds.

### Incident sequence: detect → verify → revoke → notify → export audit

```text
detect → verify (off-chain) → revoke (on-chain) → notify → export audit JSON
```

#### 1. Detect

Signals that may start an incident:

- Dashboard / indexer alert on unexpected `VerifiedEvent` or address change
- Contributor or partner report of a bad payout address
- Security review finding a mapping that should not be trusted
- Horizon / RPC monitoring showing anomalous registry activity (see
  [DEPLOYMENT.md](DEPLOYMENT.md))

Record: UTC time, username, Stellar address, ledger/tx if known, reporter.

#### 2. Verify (off-chain confirmation before mutate)

Before calling revoke on mainnet:

```bash
export NETWORK=mainnet
export CONTRACT_ID=<C...>          # from deployments/mainnet.json
export SOURCE=<admin-or-verifier>  # Stellar CLI identity name
export GITHUB_USER=<username>
export CALLER=$(stellar keys address "$SOURCE")

# Read current record (read-only simulation)
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_address --github-username "$GITHUB_USER"

# Optional: aggregate counts before mutate
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_stats
```

Confirm:

- [ ] Username is registered (`get_address` returns a record, not empty).
- [ ] `verified` is currently `true` (otherwise revoke returns `NotVerified`).
- [ ] Stellar address matches the compromised/invalid mapping under investigation.
- [ ] You are not about to revoke the wrong case-variant of the username.

#### 3. Revoke (on-chain)

**Simulate first** (omit `--send=yes`):

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- revoke_verification \
  --caller "$CALLER" \
  --github-username "$GITHUB_USER"
```

**Submit** only after a successful simulation:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  --send=yes \
  -- revoke_verification \
  --caller "$CALLER" \
  --github-username "$GITHUB_USER"
```

Makefile convenience (ensure `CALLER` is passed — the stock
`invoke-revoke-verification` target historically omitted `--caller`; prefer the
explicit invoke above, matching [ABI.md](ABI.md)):

```bash
# Equivalent explicit form used by operators during incidents:
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network mainnet \
  --send=yes \
  -- revoke_verification \
  --caller "$CALLER" \
  --github-username "$GITHUB_USER"
```

Post-revoke checks:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_address --github-username "$GITHUB_USER"
# Expect verified == false; stellar_address unchanged.

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_stats
# Expect verified decreased by 1 vs pre-revoke snapshot (when the record was verified).
```

Successful revoke publishes `VerificationRevokedEvent`
(`github_username` topic, `stellar_address`, `timestamp`). Indexers should pick
this up; see [EVENT_INDEXING.md](EVENT_INDEXING.md).

#### 4. Notify (communications)

Use the [comms checklist](#communications-template) below. Notify before or
immediately after the successful mainnet revoke, depending on severity and
disclosure policy. Do not delay revoke waiting on long approval threads when
compromise is confirmed.

#### 5. Export audit JSON

Capture an immutable operator packet for compliance. See
[Post-incident audit export](#post-incident-audit-export-checklist).

### Expected errors and operator response

Mapped from `ContractError` / [ABI.md](ABI.md) / `src/error.rs`:

| Error | Code | Meaning during incident | Operator action |
|-------|------|-------------------------|-----------------|
| `NotInitialized` | 2 | Wrong contract ID or deploy never initialized | Stop. Re-check `CONTRACT_ID` / network. |
| `NotAuthorized` | 3 | Caller is not admin and not Verifier | Stop. Switch to admin or Verifier identity; confirm `set_role` state with `get_role`. |
| `NotRegistered` | 4 | Username has no registry record | Stop revoke. Re-check spelling/case; investigate whether mapping was already removed. |
| `NotVerified` | 6 | Record exists but `verified` is already false | No mutate needed for trust-stop. Document as already revoked/unverified; still run notify/audit if the incident is real. |
| `Paused` | 7 | Contract mutations are paused | Coordinate with admin: either `unpause` briefly for revoke, or keep paused and document that trust is already frozen globally. |
| Auth trap (`require_auth`) | — | Signature / identity mismatch | Confirm CLI `--source-account` matches `--caller` G-address. |

Failed calls do **not** emit `VerificationRevokedEvent` (see ABI event notes).

### Revoke vs remove (warning)

> **Do not use `remove` when `revoke_verification` is sufficient.**

| | `revoke_verification` | `remove` |
|--|----------------------|----------|
| Effect | Sets `verified = false`; keeps registration | Deletes the record and index entry |
| Counters | Decrements `verified` only | Decrements `total`; decrements `verified` if it was verified |
| Event | `VerificationRevokedEvent` | `RemovedEvent` |
| Incident fit | **Default** — stops trust quickly | Destructive; harder to reconstruct; use only when the mapping must leave the registry |
| Auth | Admin or Verifier | Admin or registrant |

Removing during an active compromise investigation can erase useful on-chain
context (registered address still visible after revoke). Prefer revoke first;
schedule `remove` later only if process requires it.

### Communications template

Copy and fill:

```text
Subject: [TrustBridge MAINNET] Verification revoke — <github_username>

Status: CONFIRMED / INVESTIGATING
Severity: SEV-1 (trust stop) / SEV-2
UTC start: <ISO-8601>
Contract: <CONTRACT_ID>
Network: mainnet
Username: <github_username>
Stellar address: <G...>
Action taken: revoke_verification (NOT remove)
Tx hash / ledger: <...>
Event: VerificationRevokedEvent
Verified count before → after: <n> → <m>

Impact: Mapping remains registered but must NOT be treated as verified for
payouts or trust decisions until re-verified off-chain.

Next steps:
- [ ] Dashboard/indexer consumers honor VerificationRevokedEvent
- [ ] Re-verify GitHub identity off-chain before any future verify()
- [ ] Complete audit export packet (see ADMIN_RUNBOOK)
- [ ] Schedule post-incident review
```

Stakeholder checklist:

- [ ] Security / on-call
- [ ] Dashboard & indexer owners
- [ ] Payout / rewards operators
- [ ] Affected contributor (when disclosure policy allows)
- [ ] Maintainers / admin multisig signers

### Post-incident audit export checklist

Assemble a single directory (or ticket attachment) that does **not** mutate
chain state:

- [ ] Pre-revoke `get_address` JSON for the username
- [ ] Pre-revoke `get_stats` JSON
- [ ] Invoke simulation output (if retained)
- [ ] Successful revoke transaction hash + ledger sequence
- [ ] Post-revoke `get_address` JSON (`verified: false`)
- [ ] Post-revoke `get_stats` JSON
- [ ] Horizon/RPC event fetch for `VerificationRevokedEvent` on that tx
- [ ] Optional admin export snapshot for context (prefer paginated export for
      large registries — see [DASHBOARD_SYNC.md](DASHBOARD_SYNC.md)):

```bash
# Admin-only full export (small registries only)
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_all_registered
```

- [ ] Comms message as sent (final text)
- [ ] Timeline: detect → verify → revoke → notify → export (UTC)
- [ ] Explicit note: **`remove` was not used** (or justification if it was)

Store the packet in your compliance archive. On-chain events remain the
append-only source of truth; this export is the operator-facing bundle.

### Tabletop walkthrough (15 minutes)

1. Pick a testnet/futurenet verified username (never rehearse first on mainnet).
2. Snapshot `get_address` + `get_stats`.
3. Simulate `revoke_verification` with wrong `--caller` → expect auth failure /
   `NotAuthorized`.
4. Simulate with correct caller → success.
5. Send revoke; confirm `verified == false` and `VerificationRevokedEvent`.
6. Attempt second revoke → expect `NotVerified`.
7. Fill the comms template and audit checklist once as a dry run.
8. Confirm the team verbalizes: **revoke first, remove only if required later.**
