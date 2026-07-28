# Contract Health Check / RPC Probe Design

Operational liveness and readiness probes for TrustBridge deployments, built
from **existing read-only contract methods** plus optional off-chain Horizon /
indexer checks.

Related docs: [ARCHITECTURE](ARCHITECTURE.md) · [DEPLOYMENT](DEPLOYMENT.md) ·
[ABI](ABI.md) · [EVENT_INDEXING](EVENT_INDEXING.md) · [SECURITY](SECURITY.md)

> **No new on-chain `health` function is required.** Compose probes from
> methods already in the ABI. Status pages and SRE monitors should call these
> via Stellar RPC (simulation / `stellar contract invoke` without `--send=yes`).

---

## Liveness vs readiness vs indexer health

| Signal | Question | Typical probe |
|--------|----------|---------------|
| **Contract liveness** | Can RPC reach the contract WASM and return a response? | Any successful read (`version`, `get_stats`) |
| **Contract readiness** | Is the instance initialized, admin set, and stats sane for serving traffic? | Init-gated call + admin check + stats invariants |
| **Event / indexer health** | Are off-chain consumers caught up with ledger events? | Horizon / indexer lag (off-chain) |

A live but **not ready** contract (deployed, never initialized) must not be
routed to by payout dashboards. A ready contract with a **lagging indexer**
can still serve authoritative on-chain reads; dashboards that depend on events
should show degraded.

---

## Probe sequence (recommended order)

Run probes in this order. Stop or mark degraded as soon as a hard failure
appears; later probes may be misleading if earlier ones failed.

```text
1. RPC reachability      → version / get_stats responds
2. Initialized?          → init-gated call succeeds (not NotInitialized)
3. Admin set?            → has_admin_role(admin_address) == true
4. Stats sane?           → get_stats invariants
5. Pause state           → is_paused (informational / readiness policy)
6. Recent event activity → optional off-chain Horizon/indexer lag note
```

### 1. Liveness — RPC reachability

```bash
export NETWORK=mainnet   # or testnet / futurenet
export CONTRACT_ID=<C...>
export SOURCE=default    # any funded identity for simulation

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- version

# or:
make invoke-version CONTRACT_ID="$CONTRACT_ID" NETWORK="$NETWORK" SOURCE="$SOURCE"

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_stats

make invoke-stats CONTRACT_ID="$CONTRACT_ID" NETWORK="$NETWORK" SOURCE="$SOURCE"
```

| Result | Meaning |
|--------|---------|
| Returns a version tuple / stats JSON | **Live** (RPC + contract callable) |
| RPC timeout / connection error | **Down** (infra) — not a contract logic failure |
| Contract ID unknown on network | Wrong ID or wrong network |

**Caveat:** `version` and `get_stats` do **not** prove initialization.
Uninitialized instances can still return a build fallback version and
`{ total: 0, verified: 0 }`. Always continue to probe 2.

### 2. Readiness — initialized?

Initialization is defined as instance storage having `ADMIN_KEY`
(`require_initialized` in `src/storage.rs`). Prefer an **init-gated** read
that returns `NotInitialized` when unset.

Public paginated export requires init (and respects pause):

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_public_paginated --cursor 0 --limit 1
```

| Result | Meaning |
|--------|---------|
| Returns an `ExportPage` (possibly empty `records`) | **Initialized** |
| Error `NotInitialized` (code 2) | **Not ready** — deploy finished but `initialize` never succeeded |
| Error `Paused` (code 7) | Initialized but mutations/public export gated — see probe 5 |

Admin-only alternative (requires admin auth):

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account admin \
  --network "$NETWORK" \
  -- get_registered_paginated --cursor 0 --limit 1
```

### 3. Readiness — admin set?

After init, confirm the expected admin G-address still holds admin authority:

```bash
export EXPECTED_ADMIN=G...   # from deployments/<network>.json / ops secrets

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- has_admin_role --caller "$EXPECTED_ADMIN"
# Expect: true

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_role --address "$EXPECTED_ADMIN"
# Expect: Admin (or equivalent role encoding)
```

| Result | Meaning |
|--------|---------|
| `has_admin_role` → `true` for expected admin | **Admin set** as configured |
| `false` / unexpected role | **Unhealthy config** — wrong env admin, or role drift |

There is no separate `get_admin()` export in the public ABI; ops must compare
against the known deployment admin via `has_admin_role` / `get_role`.

### 4. Readiness — stats sane?

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_stats
# Shape: { "total": <u32>, "verified": <u32> }

stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- get_verified_count
```

Healthy criteria (aligned with [REGISTRY_INVARIANTS.md](REGISTRY_INVARIANTS.md)):

| Check | Healthy | Unhealthy / degraded |
|-------|---------|----------------------|
| `verified <= total` | Pass | Fail — counter drift |
| `get_verified_count() == get_stats().verified` | Pass | Fail — invariant I3 broken |
| Fresh deploy expectation | `{0,0}` right after init | Non-zero on brand-new instance → stale storage ([SECURITY.md](SECURITY.md), [DEPLOYMENT.md](DEPLOYMENT.md)) |
| Production | `total` matches ops expectation within tolerance | Sudden unexplained drop → investigate removals / wrong contract ID |

Stats are O(1) instance counters; they do not themselves prove every persistent
record is intact. Pair with event/indexer reconciliation when investigating
drift.

### 5. Pause state (policy)

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- is_paused

# Indexer alias:
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- is_contract_paused
```

| Policy choice | Recommendation |
|---------------|----------------|
| Status page “serving reads” | `is_paused == true` may still be **ready for reads** (`get_stats`, `get_address`) but **not ready for registrations/verify** |
| “Accepting mutations” readiness | Require `is_paused == false` |

Document which definition your monitor uses.

### 6. Recent event activity (optional / off-chain)

There is **no** on-chain “last event timestamp” accessor. Treat recent activity
as an **off-chain complement**:

1. Subscribe to contract events via Horizon / RPC (`RegisteredEvent`,
   `RemovedEvent`, `VerifiedEvent`, `VerificationRevokedEvent`, pause/role
   events — see `src/events.rs` and [EVENT_INDEXING.md](EVENT_INDEXING.md)).
2. Track indexer cursor vs latest ledger.
3. Alert on lag thresholds (example assumptions — tune per env):

| Signal | Example healthy | Example degraded |
|--------|-----------------|------------------|
| Indexer lag (ledgers) | &lt; 10 | ≥ 10 |
| Time since last ingested event | Env-specific | No events + unexpected `get_stats` change |
| Horizon outage | N/A | Mark **indexer unhealthy**; contract probes may still be green |

Do not fail **contract readiness** solely because the indexer is lagging unless
your product hard-depends on events for the user-facing path.

---

## Healthy vs unhealthy summary

### Healthy (ready)

- RPC invokes succeed.
- Init-gated probe succeeds (not `NotInitialized`).
- `has_admin_role(expected_admin) == true`.
- `verified <= total` and `get_verified_count == get_stats.verified`.
- Pause state matches the monitor’s mutation/readiness policy.
- (Optional) Indexer lag within threshold.

### Unhealthy / degraded

| Failure | Operational meaning |
|---------|---------------------|
| RPC errors | Infra / network — page platform |
| `NotInitialized` | Do not route traffic; run `initialize` per [DEPLOYMENT.md](DEPLOYMENT.md) |
| Admin check false | Misconfigured monitor or serious ops incident |
| `verified > total` | Counter corruption — stop trusting stats; investigate |
| Unexpected non-zero stats on fresh deploy | Wrong contract ID / reused storage |
| Paused (if mutations required) | Degraded write path; reads may continue |
| Indexer lag only | Degraded dashboards; on-chain truth still available via RPC |

---

## Suggested monitor integration

| Component | Use |
|-----------|-----|
| Stellar RPC | Simulate the probe invokes on an interval (e.g. 1–5 min) |
| Horizon | Event stream + ledger tip for lag |
| Status page | Separate tiles: Contract live · Contract ready · Indexer lag |
| Alerting | Page on readiness failures; ticket on indexer lag |

Example composite (pseudo):

```text
liveness  = invoke(version) OK
readiness = invoke(get_public_paginated) OK
            AND has_admin_role(EXPECTED_ADMIN)
            AND stats_sane(get_stats, get_verified_count)
            AND (NOT require_unpaused OR is_paused == false)
indexer   = horizon_lag < THRESHOLD   # optional tile
```

---

## Architecture notes

Probe surface maps to modules described in [ARCHITECTURE.md](ARCHITECTURE.md):

- Storage counters / admin sentinel → readiness
- Events module → off-chain activity only
- Deployment verification steps (`get_stats` after deploy) → subset of this
  design; extend those steps with init-gated + admin checks for production
  monitors ([DEPLOYMENT.md — Verify deployment](DEPLOYMENT.md#4-verify-deployment))
