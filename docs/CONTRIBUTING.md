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

### Rustdoc

Every public function and type in `src/` that integrators interact with must have a rustdoc summary.
Follow these conventions:

- **One-line summary** first (sentence ending in `.`), then a blank line, then optional detail.
- **`# Auth`** section on any function that calls `require_auth()` — state exactly which identity must sign.
- **`# Errors`** section on every `Result`-returning function — list each `ContractError` variant that can be returned and when.
- Keep docs short and factual. Do not duplicate examples from [ABI.md](ABI.md).

Example:

```rust
/// Registers or updates a GitHub username → Stellar address mapping.
///
/// # Auth
///
/// Requires auth from `stellar_address`.
///
/// # Errors
///
/// - [`ContractError::NotInitialized`] if `initialize` has not been called.
/// - [`ContractError::Paused`] if the contract is paused.
/// - [`ContractError::InvalidUsername`] if the username fails validation.
pub fn register(env: Env, github_username: String, stellar_address: Address) -> Result<(), ContractError> {
```

#### Building the docs locally

```bash
# Open in browser (recommended for contributors)
cargo doc --no-deps --open

# Build only, no browser (useful in CI or headless environments)
cargo doc --no-deps
```

Docs are generated into `target/doc/trustbridge_contract/`. The `--no-deps` flag
skips dependencies and keeps the output focused on the contract's own API.

To also include private items (useful when exploring internals):

```bash
cargo doc --no-deps --document-private-items --open
```

#### CI doc build

`cargo doc --no-deps` runs as part of the CI quality gate. A doc build failure
(broken intra-doc links, missing `# Errors` on `Result` functions, etc.) blocks
merge just like a test failure.

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
