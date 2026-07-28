# Contributing to TrustBridge Contract

Thank you for your interest in contributing to **trustbridge-contract**! This guide covers setup, workflow, and standards.

Related docs: [README](../README.md) · [ARCHITECTURE](ARCHITECTURE.md) · [ABI](ABI.md) · [DEPLOYMENT](DEPLOYMENT.md) · [Testnet Checklist](TESTNET_CHECKLIST.md)

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

Soroban generates snapshot files in `test_snapshots/` — these are gitignored and regenerated locally.

## Writing Contract Tests

This repo uses the Soroban SDK test host. Tests live in `src/lib.rs` inside
`#[cfg(test)] mod test` for unit tests and in `tests/integration.rs` for
integration tests.

### Test host setup

Every test starts with an `Env` and a deployed contract instance:

```rust
let env = Env::default();
let admin = Address::generate(&env);
let user = Address::generate(&env);
let contract_id = env.register(TrustBridgeContract, ());
env.as_contract(&contract_id, || {
    TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
});
```

`env.mock_all_auths()` disables signature checks for the current frame, so you
can call admin-only entry points without constructing real key pairs:

```rust
env.mock_all_auths();
env.as_contract(&contract_id, || {
    TrustBridgeContract::register(env.clone(), username, user.clone()).unwrap();
});
```

When you need to test an auth failure, clear the mocked auths:

```rust
env.set_auths(&[]);
let result = client.try_register(&name, &other);
assert!(result.is_err());
```

### Common patterns in this repo

- **Storage helpers** live in `src/storage.rs`. Call them directly when you want
  to inspect or construct state without going through the public ABI.
- **Events** are asserted via `env.events().all()`. Compare against a fully
  populated event struct so topic symbols and data fields are pinned.
- **Ledger control**: `env.ledger().set_timestamp(ts)` is useful when an event
  payload includes a timestamp you want to assert exactly.

### Exemplar tests to study

| Test file | Test name | What it covers |
|---|---|---|
| `src/lib.rs` | `test_register_and_get_address_roundtrip` | Basic register + lookup |
| `src/lib.rs` | `test_removed_event_payload_is_complete` | Event topic + data shape |
| `src/lib.rs` | `test_register_transfer_requires_current_owner_auth` | Auth enforcement |
| `src/lib.rs` | `test_verifier_role_can_verify` | Role-based access |
| `tests/integration.rs` | `test_integration_full_registry_lifecycle_and_events` | End-to-end lifecycle |

### Snapshot policy

Soroban may emit snapshot files under `test_snapshots/` when an event or type
layout changes. These files are gitignored. If `cargo test` fails with a
snapshot mismatch after an intentional ABI change, delete the stale snapshot and
re-run — the test suite will regenerate it.

## Pull Request Checklist

- [ ] `make check` passes locally
- [ ] New behavior has unit tests
- [ ] Documentation updated (ABI, Architecture, README as applicable)
- [ ] No secrets or `.env` files committed
- [ ] PR description explains **why** the change is needed

## Testnet Deployment Checklist

Before promoting to futurenet or mainnet, run the testnet smoke checklist:

```bash
make testnet-checklist
```

See [TESTNET_CHECKLIST.md](TESTNET_CHECKLIST.md) for the full numbered steps.

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
