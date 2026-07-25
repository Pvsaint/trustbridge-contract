# Dashboard & Indexer Sync Guide

The TrustBridge dashboard and indexer consumers combine Soroban contract state with Horizon API checks to ensure secure, efficient payout readiness and contributor index synchronization.

## Features & Integration Overview

1. **Chunked Username Index (Issue #2)**: Contributor usernames are stored in chunked persistent vectors (100 items per chunk) to avoid storage entry size limits at scale.
2. **Paginated Cursor Export (Issue #1)**: Export endpoints (`get_registered_paginated` and `get_public_paginated`) accept a zero-based offset `cursor` and item count `limit` to retrieve records deterministically without exceeding gas or frame limits.
3. **Hardened Public Reads & Emergency Pause (Issue #3)**: `get_public_paginated` allows unauthenticated dashboard reads with capped limits (`MAX_PAGE_LIMIT = 100`) and enforces emergency contract pause states.
4. **Makefile Admin Invoke Targets (Issue #30)**: Convenient CLI commands for operators to query and manage registry state.

---

## Recommended Sync Order for Dashboard Consumers

1. **Query Paginated Records**: Fetch contributor pages starting from `cursor = 0` using `get_public_paginated` or `make invoke-public-paginated`.
2. **Normalize Handles**: Lowercase GitHub handles for consistent indexing and filtering.
3. **Horizon Verification**: For each retrieved Stellar address, query Horizon for account existence, asset trustlines, and minimum reserve balances.
4. **Cache State**: Store verified status with TTL matching dashboard refresh cycles.
5. **Re-check Before Payouts**: Re-verify contributor status immediately before signing batch payments.

---

## Makefile Admin & Sync Invoke Commands

### 1. Paginated Public Sync (Indexer / Dashboard)
```bash
make invoke-public-paginated CONTRACT_ID=C... CURSOR=0 LIMIT=20 NETWORK=testnet
```

### 2. Paginated Admin Export
```bash
make invoke-export-paginated CONTRACT_ID=C... SOURCE=admin CURSOR=0 LIMIT=50 NETWORK=testnet
```

### 3. Full Registry Export
```bash
make invoke-get-all-registered CONTRACT_ID=C... SOURCE=admin NETWORK=testnet
```

### 4. Admin Verification & Revocation
```bash
# Verify contributor
make invoke-verify CONTRACT_ID=C... SOURCE=admin GITHUB_USER=octocat NETWORK=testnet

# Revoke verification
make invoke-revoke-verification CONTRACT_ID=C... SOURCE=admin GITHUB_USER=octocat NETWORK=testnet
```

### 5. Emergency Pause Control
```bash
# Pause contract
make invoke-set-paused CONTRACT_ID=C... SOURCE=admin PAUSED=true NETWORK=testnet

# Unpause contract
make invoke-set-paused CONTRACT_ID=C... SOURCE=admin PAUSED=false NETWORK=testnet
```

### 6. Remove Contributor Registration
```bash
make invoke-remove CONTRACT_ID=C... SOURCE=admin CALLER=G... GITHUB_USER=octocat NETWORK=testnet
```

---

## Test & Failure Path Scenarios

### Success Path
- Indexer invokes `get_public_paginated` with `cursor = 0` and `limit = 20`.
- Contract returns `ExportPage` with up to 20 records, `total` count, `has_more = true`, and `next_cursor = 20`.
- Indexer iterates using `cursor = next_cursor` until `has_more = false`.

### Failure Path 1: Horizon Outage or Invalid Address
- If Horizon RPC is unreachable during payout checks, contract state serves as fallback source of truth; payouts pause safely until Horizon returns online.

### Failure Path 2: Contract Paused State
- When `set_paused(true)` is set by admin, public paginated reads and state modifications fail with `ContractError::Paused` (code `7`).
- Admin unpauses via `set_paused(false)` to restore normal operation.
