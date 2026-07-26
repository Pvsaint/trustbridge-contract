# Dashboard & Indexer Synchronization

This document describes how downstream consumers (e.g., dashboards and indexers) should sync state from the TrustBridge Soroban registry.

## Vcount Invariant Testing

During active Waves, maintainers need to ensure that the registry's aggregate counts match the actual records. The contract exposes an invariant diagnostic function for this purpose.

### Exposing the Invariant Test

Contract consumers can call the read-only `check_vcount_invariant` function to verify that `verified_count` matches the exact number of `verified == true` records in the registry. 

**Usage Example:**

```bash
stellar contract invoke --id $CONTRACT_ID \
  --source-account default \
  --network testnet \
  -- check_vcount_invariant
```

Returns `true` if the invariant holds, `false` otherwise. If it returns `false`, there might be a bug or corruption in the state updates.

### Unit Tests

Unit tests are included in `src/lib.rs` under `test_check_vcount_invariant`. They test both the success and failure paths (by intentionally introducing an artificial storage count mismatch).
