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

## Migration-window dual read

When a migration is in progress, resolve a username in this order:

1. Query the local contract first.
2. If the local lookup misses and the migration window is still open, call
  the external read stub.
3. If the stub returns an address, treat it as a candidate only until the
  local contract imports the same username.
4. Once the username exists locally, stop consulting the stub for that
  record.

The stub API shape is:

```rust
RegistryLookup {
   github_username: String,
   stellar_address: Option<String>,
   source_registry_id: String,
}
```

For test coverage, the repository includes a deterministic fixture stub with
known usernames such as `legacy-alice` and `legacy-bob`. That lets dashboard
sync tests cover the fallback path without depending on a live external
registry or extra RPC wiring.
