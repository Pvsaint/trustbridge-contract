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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{TrustBridgeContract, TrustBridgeContractClient};
    use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, Symbol};

    #[test]
    fn test_zero_stats_after_initialize() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        let client = TrustBridgeContractClient::new(&env, &contract_id);

        client.initialize(&admin);

        let stats = client.get_stats();
        assert_eq!(stats.total, 0, "Total count must be zero after initialize");
        assert_eq!(stats.verified, 0, "Verified count must be zero after initialize");

        let events = env.events().all();
        for (_, topics, _) in events.into_iter() {
            if topics.len() > 0 {
                let topic_name = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
                assert_ne!(
                    topic_name,
                    Symbol::new(&env, "RegisteredEvent"),
                    "RegisteredEvent should not be emitted on initialization"
                );
                assert_ne!(
                    topic_name,
                    Symbol::new(&env, "VerifiedEvent"),
                    "VerifiedEvent should not be emitted on initialization"
                );
                assert_ne!(
                    topic_name,
                    Symbol::new(&env, "VerificationRevokedEvent"),
                    "VerificationRevokedEvent should not be emitted on initialization"
                );
            }
        }
    }
}
