# Contributing to TrustBridge Contract

Thank you for your interest in contributing to **trustbridge-contract**! This guide covers setup, workflow, and standards.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [ABI](ABI.md) · [DEPLOYMENT](DEPLOYMENT.md)

---

## Code of Conduct

Be respectful, constructive, and inclusive. Harassment and discrimination are not tolerated. Report issues to the maintainers via GitHub.

---

## Getting Started

### 1. Fork and clone

```bash
git clone https://github.com/YOUR_USERNAME/trustbridge-contract.git
cd trustbridge-contract
```

### 2. Install dependencies

```bash
rustup target add wasm32v1-none
# Stellar CLI (optional but recommended)
cargo install --locked stellar-cli@26.1.0
```

### 3. Verify the build

```bash
make check
```

This runs formatting, clippy, tests, and contract build — the same checks as CI.

---

## Development Workflow

1. **Open an issue** for significant changes (new functions, storage layout changes, breaking ABI changes)
2. **Create a branch** from `main`:
   ```bash
   git checkout -b feat/your-feature
   ```
3. **Make changes** following the code standards below
4. **Run tests**:
   ```bash
   make test
   cargo clippy --all-targets -- -D warnings
   cargo fmt --all
   ```
5. **Open a pull request** with a clear description and test plan

---

## Code Standards

### Rust / Soroban

- Match existing module structure (`lib.rs`, `storage.rs`, `events.rs`, `error.rs`)
- Use `ContractError` for recoverable errors; `require_auth()` for auth failures
- Add unit tests in `#[cfg(test)] mod test` within `lib.rs`
- Keep functions focused; storage helpers belong in `storage.rs`
- Document non-obvious auth or storage decisions inline

### Documentation

- Update [ABI.md](ABI.md) for any interface change
- Update [ARCHITECTURE.md](ARCHITECTURE.md) for storage or auth model changes
- Update [README.md](../README.md) if user-facing behavior changes

### Commit Messages

Use clear, imperative subjects:

```
Add batch lookup helper for dashboard sync
Fix verified count on address change re-registration
Update CI to Stellar CLI 26.1.0
```

---

## Testing Guidelines

Required test coverage for new behavior:

| Scenario | Expected |
|----------|----------|
| Register + lookup | Roundtrip returns correct record |
| Unauthorized removal | Non-owner gets `NotAuthorized` |
| Admin-only functions | Non-admin auth fails |
| Re-registration | Record updates; verified resets on address change |
| Stats | Counts increment/decrement correctly |

Run tests:

```bash
cargo test
```

---

## Snapshot Stabilization Policy

### What are snapshots?

Soroban's test environment can emit ledger/env state dumps into `test_snapshots/`.
These are machine-generated files that represent a serialized snapshot of the
simulated Soroban environment after a test sequence runs.

### Are snapshots committed?

**No.** `test_snapshots/` is listed in `.gitignore` and is intentionally excluded
from version control. This is deliberate:

- Snapshots are large, binary-adjacent files that produce high-noise diffs.
- They are fully reproducible by running `cargo test` locally.
- Committing them would cause spurious PR noise any time the SDK or env changes.

### When should you use snapshot-based tests?

Prefer **assert-based tests** in almost all cases:

| Use case | Recommended approach |
|----------|----------------------|
| Auth enforcement (`require_auth`) | `assert_eq!(result, Err(ContractError::NotAuthorized))` |
| Error codes | `assert_eq!(err.code(), 3)` |
| Storage round-trips | Direct `get_address` / `get_stats` assertions |
| Event emission | Assert on `env.events().all()` |
| Large `Env` state dumps | Snapshot acceptable as a last resort |

Only reach for snapshots when you need to capture a full environment dump that
would be impractical to assert field-by-field (e.g. a complex multi-step migration
sequence). If the assertion can be written explicitly, write it explicitly.

### How to refresh snapshots

If a code change legitimately alters snapshot output (e.g. a storage layout
migration), regenerate them locally with:

```bash
cargo test -- --include-ignored
```

Or for a specific test:

```bash
cargo test <test_name> -- --include-ignored
```

The regenerated files will appear in `test_snapshots/`. They are not committed;
each developer regenerates them from the current source.

### Reviewing snapshot diffs in PRs

Because `test_snapshots/` is gitignored, snapshot files will never appear in a PR
diff. If a PR author mentions a snapshot change:

1. Check out the branch locally and run `cargo test`.
2. Inspect the generated files in `test_snapshots/` for unexpected storage key
   additions, removed fields, or size regressions.
3. If the snapshot change is intentional (schema migration, new field), the PR
   description must explain the change and include before/after output or
   a brief diff summary in prose.

### CI behavior

CI does **not** compare snapshots against a baseline — snapshots are gitignored
and therefore not present in the checkout. CI validates correctness through
`cargo test`, which must pass with zero failures. If your test relies on a
snapshot file being present, the test must either regenerate it at the start of
the run or be converted to an assert-based test.

**Summary:** assert-based tests are the default; snapshots are an escape hatch
for full-env dumps; `test_snapshots/` stays gitignored; CI stays green via
`cargo test`.

---

## Pull Request Checklist

- [ ] `make check` passes locally
- [ ] New behavior has unit tests
- [ ] Documentation updated (ABI, Architecture, README as applicable)
- [ ] No secrets or `.env` files committed
- [ ] PR description explains **why** the change is needed

---

## CI

GitHub Actions runs on every push and PR to `main`, `master`, and `develop`:

1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test`
4. `stellar contract build`

See [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).

---

## Reporting Security Issues

Do **not** open public issues for security vulnerabilities. See [SECURITY.md](SECURITY.md) for responsible disclosure instructions.

---

## Questions?

- Open a [GitHub Discussion](https://github.com/Stellar-TrustBridge/trustbridge-contract/discussions) or Issue
- Review [ARCHITECTURE.md](ARCHITECTURE.md) for design context
- Check [Stellar Soroban docs](https://developers.stellar.org/docs/build/smart-contracts/overview)

We appreciate your contributions to decentralized open-source identity on Stellar!
