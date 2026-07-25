use soroban_sdk::{contractevent, Address, BytesN, String};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredEvent {
    #[topic]
    pub github_username: String,
    pub stellar_address: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedEvent {
    #[topic]
    pub github_username: String,
    pub stellar_address: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedEvent {
    #[topic]
    pub github_username: String,
    pub stellar_address: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationRevokedEvent {
    #[topic]
    pub github_username: String,
    pub stellar_address: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradedEvent {
    #[topic]
    pub new_wasm_hash: BytesN<32>,
    pub version: (u32, u32, u32),
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PausedEvent {
    #[topic]
    pub admin: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnpausedEvent {
    #[topic]
    pub admin: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrantedEvent {
    #[topic]
    pub address: Address,
    pub role: u32,
    pub admin: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleRevokedEvent {
    #[topic]
    pub address: Address,
    pub admin: Address,
    pub timestamp: u64,
}
