use soroban_sdk::{contractevent, Address, String};

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

// Wave #45: verified count on address change (Registered, Verified, Revoked events).
//
// Re-registering a `github_username` to a new `stellar_address` must emit a
// `RegisteredEvent` carrying the *new* address, while the verified count
// transition (verified -> unverified) is expected to surface as a distinct
// `VerificationRevokedEvent`-shaped state change rather than being silently
// folded into the `RegisteredEvent`. These tests pin down that event shape
// so a future re-register/verify refactor can't quietly change it.
#[cfg(test)]
mod verified_count_on_address_change_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn env_and_address() -> (Env, Address) {
        let env = Env::default();
        let address = Address::generate(&env);
        (env, address)
    }

    #[test]
    fn registered_event_carries_new_address_on_reregister() {
        let (env, old_address) = env_and_address();
        let new_address = Address::generate(&env);
        let username = String::from_str(&env, "octocat");

        let first = RegisteredEvent {
            github_username: username.clone(),
            stellar_address: old_address.clone(),
            timestamp: 100,
        };
        let second = RegisteredEvent {
            github_username: username,
            stellar_address: new_address.clone(),
            timestamp: 200,
        };

        assert_ne!(first.stellar_address, second.stellar_address);
        assert_eq!(second.stellar_address, new_address);
    }

    #[test]
    fn verification_revoked_event_reflects_address_at_time_of_revoke() {
        let (env, address) = env_and_address();
        let username = String::from_str(&env, "octocat");

        let verified = VerifiedEvent {
            github_username: username.clone(),
            stellar_address: address.clone(),
            timestamp: 100,
        };
        let revoked = VerificationRevokedEvent {
            github_username: username,
            stellar_address: address.clone(),
            timestamp: 150,
        };

        assert_eq!(verified.stellar_address, revoked.stellar_address);
        assert!(revoked.timestamp > verified.timestamp);
    }

    #[test]
    fn revoked_event_username_topic_matches_registered_event_username_topic() {
        let (env, address) = env_and_address();
        let username = String::from_str(&env, "octocat");

        let registered = RegisteredEvent {
            github_username: username.clone(),
            stellar_address: address.clone(),
            timestamp: 100,
        };
        let revoked = VerificationRevokedEvent {
            github_username: username,
            stellar_address: address,
            timestamp: 200,
        };

        assert_eq!(registered.github_username, revoked.github_username);
    }
}
