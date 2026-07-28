use soroban_sdk::contracterror;

/// Errors returned by contract entry points.
///
/// Each variant maps to a stable `u32` code (see `code()` / `from_code()`).
/// Off-chain consumers such as the dashboard and indexer use these codes to
/// classify failed invocations without depending on the Rust enum layout.
///
/// | Code | Variant | Raised by |
/// |------|---------|-----------|
/// | 1 | `AlreadyInitialized` | `initialize` |
/// | 2 | `NotInitialized` | any function called before `initialize` |
/// | 3 | `NotAuthorized` | `remove`, `verify`, `revoke_verification`, role functions |
/// | 4 | `NotRegistered` | `remove`, `verify`, `revoke_verification` |
/// | 5 | `AlreadyVerified` | `verify` |
/// | 6 | `NotVerified` | `revoke_verification` |
/// | 7 | `Paused` | any state-mutating call while paused |
/// | 8 | `CooldownActive` | `upgrade` |
/// | 9 | `InvalidVersion` | `migrate` |
/// | 10 | `InvalidRole` | `set_role` |
/// | 11 | `InvalidUsername` | `register` |
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// A function was called before `initialize`.
    NotInitialized = 2,
    /// The caller does not have the required role or does not own the resource.
    NotAuthorized = 3,
    /// The referenced `github_username` is not registered.
    NotRegistered = 4,
    /// `verify` was called on a username that is already verified.
    AlreadyVerified = 5,
    /// `revoke_verification` was called on a username that is not verified.
    NotVerified = 6,
    /// A state-mutating function was called while the contract is paused.
    Paused = 7,
    /// `upgrade` was called before the cooldown period elapsed.
    CooldownActive = 8,
    /// `migrate` was called with a version that is not strictly greater than the current one.
    InvalidVersion = 9,
    /// `set_role` was called with an unrecognised role discriminant.
    InvalidRole = 10,
    /// The supplied GitHub username is empty, longer than
    /// `utils::MAX_USERNAME_LEN`, or contains characters GitHub does not allow.
    InvalidUsername = 11,
}

impl ContractError {
    pub fn code(self) -> u32 {
        self as u32
    }

    /// Reverse of `code()`: maps a raw u32 (e.g. decoded from a failed
    /// invocation's XDR result by a dashboard or indexer) back to the typed
    /// variant. Returns `None` for codes that don't correspond to a variant,
    /// so callers don't need to keep their own copy of this table in sync.
    pub fn from_code(code: u32) -> Option<ContractError> {
        match code {
            1 => Some(ContractError::AlreadyInitialized),
            2 => Some(ContractError::NotInitialized),
            3 => Some(ContractError::NotAuthorized),
            4 => Some(ContractError::NotRegistered),
            5 => Some(ContractError::AlreadyVerified),
            6 => Some(ContractError::NotVerified),
            7 => Some(ContractError::Paused),
            8 => Some(ContractError::CooldownActive),
            9 => Some(ContractError::InvalidVersion),
            10 => Some(ContractError::InvalidRole),
            11 => Some(ContractError::InvalidUsername),
            _ => None,
        }
    }
}

// Wave #42: ContractError code mapping for register / verify / remove / export
// consumers (dashboard, indexer, off-chain tooling) that need stable u32 codes
// without depending on the Rust enum layout.
//
// | Code | Variant             | Raised by                          |
// |------|----------------------|-------------------------------------|
// | 1    | AlreadyInitialized   | initialize                         |
// | 2    | NotInitialized       | register, remove, get_all_registered, verify, revoke_verification |
// | 3    | NotAuthorized        | remove, verify, revoke_verification |
// | 4    | NotRegistered        | remove, verify, revoke_verification |
// | 5    | AlreadyVerified      | verify                             |
// | 6    | NotVerified          | revoke_verification                |
// | 7    | Paused               | any state-mutating call while paused |
// | 8    | CooldownActive       | upgrade                            |
// | 9    | InvalidVersion       | migrate                            |
// | 10   | InvalidRole          | set_role                           |
// | 11   | InvalidUsername      | register                           |
//
// `ContractError::from_code` is the reverse of this table for off-chain
// consumers decoding a raw error code back into a typed variant.
//
// Tests covering this mapping live in `src/lib.rs`
// (`test_error_codes_match_repr`, `test_from_code_round_trips_all_variants`,
// `test_from_code_unknown_returns_none`).
