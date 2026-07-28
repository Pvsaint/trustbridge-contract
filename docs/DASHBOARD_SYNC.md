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

## Paginated registry reads (Wave #41 / Issue #143)

`get_all_registered` returns the entire index in one call, which doesn't
scale as the registry grows. Prefer cursor pagination for 100+ contributors
(see also [issue #1](https://github.com/Stellar-TrustBridge/trustbridge-contract/issues/1)).

### Limit constants (`src/storage.rs`)

| Constant | Value | Behavior |
|----------|------:|----------|
| `DEFAULT_PAGE_LIMIT` | `20` | Used when the caller passes `limit = 0` |
| `MAX_PAGE_LIMIT` | `100` | Hard upper bound per invoke |

Over-limit requests are **clamped** to `MAX_PAGE_LIMIT` (not rejected). Admin
authorization on `get_registered_paginated` is unchanged.

### Cursor / page semantics

`get_registered_paginated(cursor, limit)` and `get_public_paginated(cursor, limit)`
return `ExportPage`:

| Field | Meaning |
|-------|---------|
| `records` | Page of `(github_username, ContributorRecord)` |
| `next_cursor` | `Some(offset)` for the next page, or `None` when exhausted |
| `total` | Current registry `count` |
| `has_more` | `true` iff `next_cursor` is `Some` |

`cursor` is a **zero-based index offset** into the username index (not a opaque
token). Exhaustion: `has_more == false` and `next_cursor == None` (also when
`cursor >= total`).

### Consumer loop (fetch → process → next cursor → until exhausted)

```text
cursor = 0
loop:
  page = get_registered_paginated(cursor, limit)   # admin
        # or get_public_paginated(cursor, limit)   # public, respects pause
  process(page.records)
  if not page.has_more or page.next_cursor is None:
    break
  cursor = page.next_cursor
```

```bash
# Admin page (auth required)
make invoke-export-paginated CONTRACT_ID=$ID SOURCE=admin CURSOR=0 LIMIT=100

# Public page (no admin auth; fails if paused)
make invoke-public-paginated CONTRACT_ID=$ID CURSOR=0 LIMIT=100
```

Do **not** use `get_all_registered` once the registry approaches the ~100
ledger-entry footprint ceiling; page until exhausted instead. ABI details:
[ABI.md — Paginated export](ABI.md#paginated-export-issue-1--143).

Related unit tests in `src/lib.rs`:

- `test_paginated_export_at_max_page_limit`
- `test_paginated_export_over_max_page_limit_clamps`
- `test_get_registered_page_paginates_and_gates_on_admin`
