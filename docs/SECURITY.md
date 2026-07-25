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
| Unauthorized admin actions | `admin.require_auth()` on `verify` and `get_all_registered` |
| Double initialization | `AlreadyInitialized` error |
| Malformed or oversized username input | `InvalidUsername` error, checked before auth and before any write |
| Counter drift from rejected calls | Invariant property fuzzing, see [REGISTRY_INVARIANTS](REGISTRY_INVARIANTS.md) |
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

The admin address is **immutable** after `initialize`. Recommendations:

- Use a **multisig** or **smart account** as the admin G-address
- Never commit private keys or seed phrases
- Rotate operational keys via deploying a new contract instance if admin is compromised (no on-chain admin transfer in v0.1)

---

## Registration Integrity

- Registering a username requires the Stellar address owner to sign
- Re-registration with a new address resets verification status
- There is no on-chain proof of GitHub ownership at registration time — verification is a separate admin step

---

## Input Validation

`register` validates the username **before** `require_auth()` and before any
storage write. The order matters: a malformed call is rejected at the cheapest
point, no signature is spent on it, and no counter or index entry moves.

| Rule | Value |
|------|-------|
| Length | 1 to 39 characters (GitHub's own cap) |
| Allowed characters | `a-z`, `A-Z`, `0-9`, `-`, `_` |
| First and last character | Must be alphanumeric |

Rejection returns `InvalidUsername` (code 7).

Validation lives in `src/utils.rs` and works entirely on a fixed 39-byte stack
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

## Storage TTL

Persistent entries on Stellar mainnet have a **time-to-live (TTL)**. If entries expire, data may become unavailable until extended.

Operational teams should:

1. Monitor entry TTL via RPC
2. Run periodic TTL extension via Stellar CLI (`stellar contract extend`)
3. Document extension cadence in deployment runbooks

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

## Audit Status

This contract has **not** been formally audited. Use at your own risk on mainnet until an audit is completed.

For production deployments, consider:

- Independent security audit
- Bug bounty program
- Staged rollout on testnet/futurenet first
