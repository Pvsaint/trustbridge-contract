# Dashboard & Indexer Sync Guide

The TrustBridge dashboard and indexer consumers combine Soroban contract state with Horizon API checks to ensure secure, efficient payout readiness and contributor index synchronization.

## ABI Event Reference

All contract events are documented with their topic and data field layouts in
[docs/ABI.md#events](ABI.md#events). Indexers should use that section as the
source of truth for topic symbols, field names, and types — mismatches between
docs and on-chain `#[contractevent]` definitions are tracked as documentation
bugs.

Key events to watch:

| Event | Topic symbol | Key data fields |
|---|---|---|
| `RegisteredEvent` | `registered_event` | `stellar_address`, `timestamp` |
| `VerifiedEvent` | `verified_event` | `stellar_address`, `timestamp` |
| `VerificationRevokedEvent` | `verification_revoked_event` | `stellar_address`, `timestamp` |
| `RemovedEvent` | `removed_event` | `stellar_address`, `timestamp` |
| `UpgradedEvent` | `upgraded_event` | `version`, `timestamp` |
| `PausedEvent` / `UnpausedEvent` | `paused_event` / `unpaused_event` | `timestamp` |
| `RoleGrantedEvent` / `RoleRevokedEvent` | `role_granted_event` / `role_revoked_event` | `role`, `admin`, `timestamp` / `admin`, `timestamp` |

> **Note:** `RoleRevokedEvent` does **not** include the `role` field in its data
> payload. If your indexer needs to know which role was revoked, correlate the
> revocation with the most recent `RoleGrantedEvent` for that address.

## Features & Integration Overview

1. **Chunked Username Index (Issue #2)**: Contributor usernames are stored in chunked persistent vectors (100 items per chunk) to avoid storage entry size limits at scale.
2. **Paginated Cursor Export (Issue #1)**: Export endpoints (`get_registered_paginated` and `get_public_paginated`) accept a zero-based offset `cursor` and item count `limit` to retrieve records deterministically without exceeding gas or frame limits.
3. **Hardened Public Reads & Emergency Pause (Issue #3)**: `get_public_paginated` allows unauthenticated dashboard reads with capped limits (`MAX_PAGE_LIMIT = 100`) and enforces emergency contract pause states.
4. **Makefile Admin Invoke Targets (Issue #30)**: Convenient CLI commands for operators to query and manage registry state.

Contract verification proves the registry entry was approved; Horizon readiness proves the address can receive the selected asset.

## has_record lookup optimization (Wave #40)

`has_record(github_username) -> bool` is now exposed as a contract entry
point. Dashboard and indexer consumers that only need an existence check
(e.g. "is this username already registered?" during a form validation, or a
membership check while paging through webhook events) should call it instead
of `get_address`:

- `has_record` avoids deserializing the full `ContributorRecord`.
- `get_address` should still be used whenever the caller actually needs
  `stellar_address`, `registered_at`, or `verified`.

Tests for this behavior live alongside the contract in `src/lib.rs`
(`test_has_record_reflects_registration_state`) and `src/storage.rs`
(`test_has_record_true_after_set_record`).

## Cross-repo E2E demo

Sibling TrustBridge repos can verify their integration against a live testnet
contract using the bundled E2E demo:

```bash
CONTRACT_ID=C... \
ADMIN=G... \
E2E_STELLAR_ADDR=G... \
SOURCE=demo \
./scripts/demo_e2e.sh
```

Or via Make:

```bash
make demo-e2e CONTRACT_ID=C... ADMIN=G... E2E_STELLAR_ADDR=G... SOURCE=demo
```

The script walks `register → verify → lookup → export` and prints each step.
It fails fast on any nonzero invoke and never touches mainnet. Secrets stay in
env; nothing is committed.

## Paginated registry reads (Wave #41)

`get_all_registered` returns the entire index in one call, which doesn't
scale as the registry grows. Use `get_registered_page(offset, limit)`
instead when syncing incrementally — it walks the same admin-gated index but
in bounded chunks, so a dashboard/indexer sync job can page through without
risking a resource-limit failure on a large registry. See
`test_get_registered_page_paginates_and_gates_on_admin` in `src/lib.rs`.

## GDPR Data Export & Erasure

To assist with privacy and GDPR compliance, the contract provides hooks and patterns for exporter/erasure processes.

### 1. Data Inventory (On-chain Fields)
The following fields are stored for each contributor within `ContributorRecord` in persistent storage:
- `stellar_address` (`Address`): The registered Stellar G-address.
- `registered_at` (`u64`): Ledger timestamp of the registration/update.
- `verified` (`bool`): Boolean indicating if the off-chain check was completed.

**Privacy Note:** No personal identifiable information (PII) such as email, phone, name, or GitHub profile link is stored on-chain.

### 2. GDPR Export Hook
Integrators can retrieve a user's on-chain dataset via `get_address`:
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account deployer \
  --network testnet \
  -- get_address --github-username <username>
```

Admins can perform full or chunked exports to back up the registry state for backups or migration audits:
- **Full Export**: `get_all_registered` (Admin-only)
- **Paginated Export**: `get_registered_paginated(cursor, limit)` (Admin-only, retrieves records as a JSON bundle)

### 3. Right to Erasure (Deletion Hook)
To fulfill erasure requests under GDPR, the registrant or the admin can invoke the `remove` function:
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source-account registrant-or-admin \
  --network testnet \
  --send=yes \
  -- remove --caller <caller_address> --github-username <username>
```
Calling `remove`:
- Deletes the `ContributorRecord` from persistent storage.
- Cleans up the username from the index list chunks.
- Decrements the active registration counter and verified counter (if verified).
- Emits a `RemovedEvent`.
