use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    NotRegistered = 4,
    AlreadyVerified = 5,
    NotVerified = 6,
    Paused = 7,
    CooldownActive = 8,
    InvalidVersion = 9,
    InvalidRole = 10,
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
// | 7    | Paused               | register, remove, verify, revoke_verification, set_role, remove_role, upgrade, migrate |
// | 8    | CooldownActive       | upgrade                            |
// | 9    | InvalidVersion       | migrate                            |
// | 10   | InvalidRole          | set_role                           |
//
// `ContractError::from_code` is the reverse of this table for off-chain
// consumers decoding a raw error code back into a typed variant.
//
// Tests covering this mapping live in `src/lib.rs`
// (`test_error_codes_match_repr`, `test_from_code_round_trips_all_variants`,
// `test_from_code_unknown_returns_none`).
