# Dashboard & Indexer Sync Guide

The TrustBridge dashboard and indexer consumers combine Soroban contract state with Horizon API checks to ensure secure, efficient payout readiness and contributor index synchronization.

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

## Paginated registry reads (Wave #41)

`get_all_registered` returns the entire index in one call, which doesn't
scale as the registry grows. Use `get_registered_page(offset, limit)`
instead when syncing incrementally — it walks the same admin-gated index but
in bounded chunks, so a dashboard/indexer sync job can page through without
risking a resource-limit failure on a large registry. See
`test_get_registered_page_paginates_and_gates_on_admin` in `src/lib.rs`.

## Verified flags in exports (Issue #96)

`get_all_registered` and `get_registered_page` return `(github_username,
stellar_address)` pairs only — no `verified` bit. A dashboard that needs to
know verification status alongside the address should use one of the two
`ExportPage`-returning calls instead, both of which carry the full
`ContributorRecord` (including `verified`) per entry:

| Function | Auth | Use when |
|---|---|---|
| `get_registered_paginated(cursor, limit)` | Admin | Operator/admin-side sync jobs that already hold the admin key |
| `get_public_paginated(cursor, limit)` | None | Public dashboard reads with no admin key available |

Both accept a zero-based `cursor` and a `limit` (0 defaults to 20, capped at
100), and return `{ records, next_cursor, total, has_more }`. Page forward by
passing the previous response's `next_cursor` back in as `cursor` until
`has_more` is `false`.

Tests covering empty, all-unverified, and mixed registries live in
`src/lib.rs`: `test_issue_96_paginated_export_verified_flags_empty_registry`,
`test_issue_96_paginated_export_verified_flags_all_unverified`,
`test_issue_96_paginated_export_verified_flags_mixed_registry`.

## Multi-user register sequence (Issue #94)

Reference fixture for integrators wiring a sync worker against `register`:
three users, registered one at a time, with the expected event and stats
progression at each step.

| Step | Call | `RegisteredEvent` topic | `get_stats().total` after |
|---|---|---|---|
| 1 | `register("alice", addr1)` | `alice` | 1 |
| 2 | `register("bob", addr2)` | `bob` | 2 |
| 3 | `register("carol", addr3)` | `carol` | 3 |

Each `register` call publishes exactly one `RegisteredEvent` (topic:
`github_username`; data: `stellar_address`, `timestamp`), and `get_stats().total`
increments by exactly one per step while `verified` stays at 0 throughout,
since none of the three have been verified. After all three, every username
resolves through `get_address` and `has_record`, and the entry count in
`get_all_registered` / the paginated exports matches.

Automated coverage: `test_issue_94_multi_user_register_sequence` in
`src/lib.rs`.
