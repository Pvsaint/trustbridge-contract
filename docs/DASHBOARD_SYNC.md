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

---

## Development Webhook Payload (Proposed)

**STATUS: NOT IMPLEMENTED — Development proposal only**

This section documents a proposed JSON payload format for development webhooks that could translate contract events into HTTP callbacks for dashboard prototypes. This is **not production infrastructure** and is provided solely for future development planning.

### Purpose

Enable local development and testing of dashboard integrations by providing a standardized webhook payload format for each registry event type. Future implementations may change these formats based on actual requirements.

### Event Payload Formats

All webhook payloads follow a common structure with event-specific fields:

#### Base Payload Structure

```json
{
  "event": "<event_name>",
  "ledger": 123456,
  "timestamp": "2026-07-28T12:34:56Z",
  "transaction_id": "abc123...",
  "version": 1
}
```

#### RegisteredEvent

Emitted when a contributor registers their GitHub username with a Stellar address.

```json
{
  "event": "registered",
  "ledger": 123456,
  "timestamp": "2026-07-28T12:34:56Z",
  "transaction_id": "abc123...",
  "github_username": "octocat",
  "stellar_address": "GBXXXX...",
  "version": 1
}
```

#### RemovedEvent

Emitted when a registration is deleted (by contributor or admin).

```json
{
  "event": "removed",
  "ledger": 123457,
  "timestamp": "2026-07-28T13:45:12Z",
  "transaction_id": "def456...",
  "github_username": "octocat",
  "stellar_address": "GBXXXX...",
  "version": 1
}
```

#### VerifiedEvent

Emitted when an admin marks a contributor as verified after off-chain identity confirmation.

```json
{
  "event": "verified",
  "ledger": 123458,
  "timestamp": "2026-07-28T14:20:33Z",
  "transaction_id": "ghi789...",
  "github_username": "octocat",
  "stellar_address": "GBXXXX...",
  "version": 1
}
```

#### VerificationRevokedEvent

Emitted when an admin revokes a contributor's verification status.

```json
{
  "event": "verification_revoked",
  "ledger": 123459,
  "timestamp": "2026-07-28T15:10:22Z",
  "transaction_id": "jkl012...",
  "github_username": "octocat",
  "stellar_address": "GBXXXX...",
  "version": 1
}
```

#### UpgradedEvent

Emitted when the contract WASM is upgraded.

```json
{
  "event": "upgraded",
  "ledger": 123460,
  "timestamp": "2026-07-28T16:05:44Z",
  "transaction_id": "mno345...",
  "new_wasm_hash": "abc123def456...",
  "version_major": 1,
  "version_minor": 2,
  "version_patch": 0,
  "version": 1
}
```

#### PausedEvent

Emitted when the contract is paused by an admin (emergency stop).

```json
{
  "event": "paused",
  "ledger": 123461,
  "timestamp": "2026-07-28T17:30:11Z",
  "transaction_id": "pqr678...",
  "admin": "GAXXXX...",
  "version": 1
}
```

#### UnpausedEvent

Emitted when the contract is unpaused by an admin.

```json
{
  "event": "unpaused",
  "ledger": 123462,
  "timestamp": "2026-07-28T18:15:55Z",
  "transaction_id": "stu901...",
  "admin": "GAXXXX...",
  "version": 1
}
```

#### RoleGrantedEvent

Emitted when an admin grants a role to an address.

```json
{
  "event": "role_granted",
  "ledger": 123463,
  "timestamp": "2026-07-28T19:00:00Z",
  "transaction_id": "vwx234...",
  "address": "GCXXXX...",
  "role": 3,
  "admin": "GAXXXX...",
  "version": 1
}
```

Role values:
- `1` = Admin
- `2` = Upgrader
- `3` = Verifier

#### RoleRevokedEvent

Emitted when an admin revokes a role from an address.

```json
{
  "event": "role_revoked",
  "ledger": 123464,
  "timestamp": "2026-07-28T20:45:30Z",
  "transaction_id": "yz0567...",
  "address": "GCXXXX...",
  "admin": "GAXXXX...",
  "version": 1
}
```

### Security Considerations

**IMPORTANT: Development webhook stubs must follow these security practices:**

- **Never commit webhook URLs** — Always use environment variables (e.g., `WEBHOOK_URL`)
- **Never commit tokens or secrets** — Use environment variables (e.g., `WEBHOOK_SECRET`)
- **Implement webhook signature verification** — Recipients should validate payloads using HMAC or similar
- **Use HTTPS only** — Never send webhooks over unencrypted HTTP in any environment
- **Idempotency tokens** — Include transaction IDs to allow recipients to deduplicate events
- **Retry logic** — Future implementations should document retry backoff strategy (see issue #104)

### Future Work

This proposal does NOT include:

- Webhook delivery infrastructure
- HTTP server implementation
- Retry and backoff logic
- Idempotency guarantees beyond transaction IDs
- Production-grade monitoring and alerting
- Rate limiting or throttling

Implementers should reference:
- Issue #104 for retry and idempotency expectations
- `docs/EVENT_INDEXING.md` for event consumer best practices
- `docs/SECURITY.md` for general security guidelines

### Local Development Testing

For local testing of webhook consumers, developers may:

1. Set up a local HTTP endpoint (e.g., using `ngrok` or a simple HTTP server)
2. Configure the webhook URL via environment variable
3. Manually trigger test events or use contract test utilities
4. Validate JSON schema and payload structure
5. Test error handling and retry scenarios

**Remember:** Any local relay script or stub server is strictly for development and should be clearly marked as non-production code.
