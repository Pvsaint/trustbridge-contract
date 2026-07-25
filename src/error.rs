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
}

impl ContractError {
    pub fn code(self) -> u32 {
        self as u32
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
// | 3    | NotAuthorized        | remove                             |
// | 4    | NotRegistered        | remove, verify, revoke_verification |
// | 5    | AlreadyVerified      | verify                             |
// | 6    | NotVerified          | revoke_verification                |
//
// Tests covering this mapping live in `src/lib.rs`
// (`test_contract_error_code_mapping`, `test_remove_missing_registration_maps_to_not_registered`).
