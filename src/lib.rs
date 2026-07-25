#![no_std]

// Tests and benchmarks need allocation and stdout for reporting results. The
// contract itself stays no_std.
#[cfg(test)]
extern crate std;

mod error;
mod events;
mod storage;

pub use error::ContractError;
pub use events::{RegisteredEvent, RemovedEvent, VerifiedEvent, VerificationRevokedEvent};
pub use storage::{ContributorRecord, Stats};

use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec};

use crate::storage::{
    add_to_index, get_admin, get_count, get_index, get_record, get_stats as read_stats, has_record,
    get_verified_count as storage_get_verified_count, remove_from_index, remove_record, require_initialized, set_count,
    set_record, set_verified_count, ADMIN_KEY,
};

#[contract]
pub struct TrustBridgeContract;

#[contractimpl]
impl TrustBridgeContract {
    /// Sets the contract admin. Can only be called once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&ADMIN_KEY) {
            return Err(ContractError::AlreadyInitialized);
        }

        env.storage().instance().set(&ADMIN_KEY, &admin);
        set_count(&env, 0);
        set_verified_count(&env, 0);

        Ok(())
    }

    /// Registers or updates a GitHub username → Stellar address mapping.
    /// The caller must authenticate as `stellar_address`.
    pub fn register(
        env: Env,
        github_username: String,
        stellar_address: Address,
    ) -> Result<(), ContractError> {
        require_initialized(&env)?;
        stellar_address.require_auth();

        let timestamp = env.ledger().timestamp();
        let existing = get_record(&env, &github_username);

        let record = ContributorRecord {
            stellar_address: stellar_address.clone(),
            registered_at: timestamp,
            verified: existing
                .as_ref()
                .map(|r| r.stellar_address == stellar_address && r.verified)
                .unwrap_or(false),
        };

        if existing.is_none() {
            set_count(&env, get_count(&env).saturating_add(1));
            add_to_index(&env, &github_username);
        } else if let Some(old) = existing {
            if old.stellar_address != stellar_address && old.verified {
                set_verified_count(&env, storage_get_verified_count(&env).saturating_sub(1));
            }
        }

        set_record(&env, &github_username, &record);

        RegisteredEvent {
            github_username: github_username.clone(),
            stellar_address,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Read-only lookup. Returns `None` if the username is not registered.
    pub fn get_address(env: Env, github_username: String) -> Option<ContributorRecord> {
        if has_record(&env, &github_username) {
            get_record(&env, &github_username)
        } else {
            None
        }
    }

    /// Removes a registration. Callable by the registrant or the admin.
    ///
    /// `caller` must sign the transaction and must equal either the contract
    /// admin or the registered Stellar address for `github_username`.
    pub fn remove(env: Env, caller: Address, github_username: String) -> Result<(), ContractError> {
        require_initialized(&env)?;

        let record = get_record(&env, &github_username).ok_or(ContractError::NotRegistered)?;
        let admin = get_admin(&env)?;

        caller.require_auth();
        if caller != admin && caller != record.stellar_address {
            return Err(ContractError::NotAuthorized);
        }

        let timestamp = env.ledger().timestamp();
        let stellar_address = record.stellar_address.clone();

        remove_record(&env, &github_username);
        remove_from_index(&env, &github_username);
        set_count(&env, get_count(&env).saturating_sub(1));

        if record.verified {
            set_verified_count(&env, storage_get_verified_count(&env).saturating_sub(1));
        }

        RemovedEvent {
            github_username: github_username.clone(),
            stellar_address,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Returns the full registry. Admin-only.
    pub fn get_all_registered(env: Env) -> Result<Vec<(String, Address)>, ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        let index = get_index(&env);
        let mut result = Vec::new(&env);

        for i in 0..index.len() {
            let username = index.get(i).unwrap();
            if let Some(record) = get_record(&env, &username) {
                result.push_back((username, record.stellar_address));
            }
        }

        Ok(result)
    }

    /// Marks a contributor as verified after an off-chain GitHub identity check. Admin-only.
    pub fn verify(env: Env, github_username: String) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        let mut record = get_record(&env, &github_username).ok_or(ContractError::NotRegistered)?;

        if record.verified {
            return Err(ContractError::AlreadyVerified);
        }

        record.verified = true;
        set_record(&env, &github_username, &record);
        set_verified_count(&env, storage_get_verified_count(&env).saturating_add(1));

        let timestamp = env.ledger().timestamp();
        VerifiedEvent {
            github_username: github_username.clone(),
            stellar_address: record.stellar_address.clone(),
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Revokes verification for a registered contributor. Admin-only.
    pub fn revoke_verification(env: Env, github_username: String) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        let mut record = get_record(&env, &github_username).ok_or(ContractError::NotRegistered)?;

        if !record.verified {
            return Err(ContractError::NotVerified);
        }

        record.verified = false;
        set_record(&env, &github_username, &record);
        set_verified_count(&env, storage_get_verified_count(&env).saturating_sub(1));

        let timestamp = env.ledger().timestamp();
        VerificationRevokedEvent {
            github_username: github_username.clone(),
            stellar_address: record.stellar_address.clone(),
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Returns the verified registration count.
    pub fn get_verified_count(env: Env) -> u32 {
        storage_get_verified_count(&env)
    }

    /// Returns aggregate registration statistics.
    pub fn get_stats(env: Env) -> Stats {
        read_stats(&env)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, String};

    fn setup(env: &Env) -> (Address, Address, Address, Address) {
        let admin = Address::generate(env);
        let user = Address::generate(env);
        let other = Address::generate(env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
        });
        (admin, user, other, contract_id)
    }

    fn username(env: &Env, name: &str) -> String {
        String::from_str(env, name)
    }

    #[test]
    fn test_register_and_get_address_roundtrip() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();

            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(record.stellar_address, user);
            assert!(!record.verified);
        });
    }

    #[test]
    fn test_non_owner_cannot_remove() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();

            let result =
                TrustBridgeContract::remove(env.clone(), other.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    #[test]
    #[should_panic(expected = "Unauthorized function call for address")]
    fn test_admin_functions_reject_non_admin() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.set_auths(&[]);

        env.as_contract(&contract_id, || {
            let _ = TrustBridgeContract::get_all_registered(env.clone());
        });
    }

    #[test]
    fn test_reregistration_updates_record() {
        let env = Env::default();
        let (_admin, user, new_user, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();

            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();

            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), new_user.clone())
                .unwrap();

            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(record.stellar_address, new_user);
            assert!(!record.verified);

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 0);
        });
    }

    #[test]
    fn test_get_stats_increments_correctly() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 0);
            assert_eq!(stats.verified, 0);

            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 2);
            assert_eq!(stats.verified, 0);

            TrustBridgeContract::verify(env.clone(), username(&env, "alice")).unwrap();

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 2);
            assert_eq!(stats.verified, 1);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user2.clone(), username(&env, "bob")).unwrap();

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 1);
        });
    }

    #[test]
    fn test_initialize_only_once() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());

        env.as_contract(&contract_id, || {
            TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
            let result = TrustBridgeContract::initialize(env.clone(), admin);
            assert_eq!(result, Err(ContractError::AlreadyInitialized));
        });
    }

    #[test]
    fn test_register_requires_initialization() {
        let env = Env::default();
        let user = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::register(
                env.clone(),
                username(&env, "octocat"),
                user.clone(),
            );
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }


    #[test]
    fn test_admin_can_remove_registration() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "octocat"))
                .unwrap();

            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat"))
                .is_none());
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
        });
    }


    #[test]
    fn test_get_all_registered_returns_indexed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();

            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all.get(0).unwrap(), (username(&env, "alice"), user1));
            assert_eq!(all.get(1).unwrap(), (username(&env, "bob"), user2));
        });
    }


    #[test]
    fn test_removing_verified_record_updates_stats() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat"))
                .unwrap();

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 0);
            assert_eq!(stats.verified, 0);
        });
    }


    #[test]
    fn test_reregister_same_address_keeps_verification() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();

            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(record.verified);
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 1);
        });
    }


    #[test]
    fn test_get_address_missing_returns_none() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none());
        });
    }


    #[test]
    fn test_verify_missing_registration_fails() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), username(&env, "missing"));
            assert_eq!(result, Err(ContractError::NotRegistered));
        });
    }


    #[test]
    fn test_remove_missing_registration_fails() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "missing"));
            assert_eq!(result, Err(ContractError::NotRegistered));
        });
    }


    #[test]
    fn test_double_verify_fails() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::AlreadyVerified));
        });
    }

    #[test]
    fn test_revoke_verification_decrements_verified_count() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::revoke_verification(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }

    #[test]
    fn test_revoke_verification_nonverified_fails() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::revoke_verification(env.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotVerified));
        });
    }


    #[test]
    fn test_register_two_users_keeps_addresses() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
            assert_eq!(TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).unwrap().stellar_address, user1);
            assert_eq!(TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().stellar_address, user2);
        });
    }


    #[test]
    fn test_owner_can_remove_registration() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat")).unwrap();
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).is_none());
        });
    }


    #[test]
    fn test_readding_removed_user_increments_count() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }


    #[test]
    fn test_export_skips_removed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!(all.get(0).unwrap(), (username(&env, "bob"), user2));
        });
    }


    #[test]
    fn test_stats_empty_after_setup() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 0);
            assert_eq!(stats.verified, 0);
        });
    }


    #[test]
    fn test_removed_verified_user_can_register_unverified() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }


    #[test]
    fn test_error_codes_match_repr() {
        assert_eq!(ContractError::AlreadyInitialized.code(), 1);
        assert_eq!(ContractError::NotInitialized.code(), 2);
        assert_eq!(ContractError::NotAuthorized.code(), 3);
        assert_eq!(ContractError::NotRegistered.code(), 4);
        assert_eq!(ContractError::AlreadyVerified.code(), 5);
    }


    #[test]
    fn test_updated_registration_preserves_count() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }


    #[test]
    fn test_unverified_update_stays_unverified() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }


    #[test]
    fn test_verified_same_address_reregister_keeps_count() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 1);
        });
    }


    #[test]
    fn test_verified_address_change_decrements_verified_count() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 0);
        });
    }


    #[test]
    fn test_admin_export_empty_registry() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 0);
        });
    }


    #[test]
    fn test_removing_one_of_two_keeps_remaining_stats() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 0);
        });
    }


    #[test]
    fn test_remove_then_lookup_other_record() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
            assert_eq!(TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().stellar_address, user2);
        });
    }


    #[test]
    fn test_verify_after_reregister_new_address() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap().verified);
        });
    }


    #[test]
    fn test_repeated_missing_lookups_are_stable() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none());
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none());
        });
    }


    #[test]
    fn test_register_after_empty_export() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_all_registered(env.clone()).unwrap().len(), 0);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            assert_eq!(TrustBridgeContract::get_all_registered(env.clone()).unwrap().len(), 1);
        });
    }

    // === Invariant property fuzzing

    /// Deterministic xorshift64 generator.
    ///
    /// Seeds are fixed constants rather than clock-derived so a CI failure can
    /// be replayed locally by rerunning the same test.
    struct Prng(u64);

    impl Prng {
        fn new(seed: u64) -> Self {
            // A zero state would make xorshift emit only zeroes.
            Prng(seed | 1)
        }

        fn next_u32(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            (x >> 32) as u32
        }

        fn below(&mut self, bound: u32) -> u32 {
            self.next_u32() % bound
        }
    }

    const FUZZ_USERNAMES: [&str; 6] = ["alice", "bob", "carol", "dave", "erin", "frank"];
    const FUZZ_SEEDS: [u64; 4] = [0x5eed_0001, 0x0bad_c0de, 0xdead_beef, 0x1234_5678];
    const FUZZ_MISSING: [&str; 3] = ["ghost", "nobody", "absent"];

    #[derive(Clone, Copy)]
    struct ShadowRecord {
        address: u32,
        verified: bool,
    }

    /// Model of the registry maintained outside contract storage. Invariants
    /// are asserted against this model so a bug in the contract's own counters
    /// cannot mask itself.
    struct Shadow {
        records: [Option<ShadowRecord>; FUZZ_USERNAMES.len()],
    }

    impl Shadow {
        fn new() -> Self {
            Shadow {
                records: [None; FUZZ_USERNAMES.len()],
            }
        }

        fn total(&self) -> u32 {
            self.records.iter().filter(|r| r.is_some()).count() as u32
        }

        fn verified(&self) -> u32 {
            self.records
                .iter()
                .filter(|r| matches!(r, Some(rec) if rec.verified))
                .count() as u32
        }
    }

    fn assert_registry_invariants(
        env: &Env,
        contract_id: &Address,
        users: &[Address],
        shadow: &Shadow,
    ) {
        env.as_contract(contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());

            // I1: total tracks the number of live records.
            assert_eq!(stats.total, shadow.total());
            // I2: verified tracks the number of live records flagged verified.
            assert_eq!(stats.verified, shadow.verified());
            // I3: the standalone getter never diverges from the stats view.
            assert_eq!(
                TrustBridgeContract::get_verified_count(env.clone()),
                stats.verified
            );
            // I4: verified is a subset of total, so it can never exceed it.
            assert!(stats.verified <= stats.total);

            // I5: the export index holds exactly one entry per live record.
            let exported = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(exported.len(), stats.total);

            // I6: every username resolves to the address last registered for it.
            for (slot, name) in FUZZ_USERNAMES.iter().enumerate() {
                let key = username(env, name);
                match shadow.records[slot] {
                    Some(expected) => {
                        let record =
                            TrustBridgeContract::get_address(env.clone(), key).unwrap();
                        assert_eq!(record.stellar_address, users[expected.address as usize]);
                        assert_eq!(record.verified, expected.verified);
                    }
                    None => {
                        assert!(TrustBridgeContract::get_address(env.clone(), key).is_none());
                    }
                }
            }
        });
    }

    fn run_fuzz_session(seed: u64, steps: u32) {
        let env = Env::default();
        let (admin, user_a, user_b, contract_id) = setup(&env);
        let users = [user_a, user_b, Address::generate(&env)];

        env.mock_all_auths();

        let mut prng = Prng::new(seed);
        let mut shadow = Shadow::new();

        for _ in 0..steps {
            let slot = prng.below(FUZZ_USERNAMES.len() as u32) as usize;
            let name = username(&env, FUZZ_USERNAMES[slot]);

            match prng.below(4) {
                0 => {
                    let addr = prng.below(users.len() as u32);
                    let target = users[addr as usize].clone();
                    env.as_contract(&contract_id, || {
                        TrustBridgeContract::register(env.clone(), name.clone(), target).unwrap();
                    });

                    // Verification survives only a re-registration of the same address.
                    let carried = matches!(
                        shadow.records[slot],
                        Some(prev) if prev.address == addr && prev.verified
                    );
                    shadow.records[slot] = Some(ShadowRecord {
                        address: addr,
                        verified: carried,
                    });
                }
                1 => {
                    let result = env.as_contract(&contract_id, || {
                        TrustBridgeContract::verify(env.clone(), name.clone())
                    });

                    match shadow.records[slot] {
                        Some(rec) if !rec.verified => {
                            result.unwrap();
                            shadow.records[slot] = Some(ShadowRecord {
                                verified: true,
                                ..rec
                            });
                        }
                        Some(_) => assert_eq!(result, Err(ContractError::AlreadyVerified)),
                        None => assert_eq!(result, Err(ContractError::NotRegistered)),
                    }
                }
                2 => {
                    let result = env.as_contract(&contract_id, || {
                        TrustBridgeContract::revoke_verification(env.clone(), name.clone())
                    });

                    match shadow.records[slot] {
                        Some(rec) if rec.verified => {
                            result.unwrap();
                            shadow.records[slot] = Some(ShadowRecord {
                                verified: false,
                                ..rec
                            });
                        }
                        Some(_) => assert_eq!(result, Err(ContractError::NotVerified)),
                        None => assert_eq!(result, Err(ContractError::NotRegistered)),
                    }
                }
                _ => {
                    let result = env.as_contract(&contract_id, || {
                        TrustBridgeContract::remove(env.clone(), admin.clone(), name.clone())
                    });

                    match shadow.records[slot] {
                        Some(_) => {
                            result.unwrap();
                            shadow.records[slot] = None;
                        }
                        None => assert_eq!(result, Err(ContractError::NotRegistered)),
                    }
                }
            }

            assert_registry_invariants(&env, &contract_id, &users, &shadow);
        }
    }

    #[test]
    fn test_fuzz_invariants_hold_across_random_operation_sequences() {
        for seed in FUZZ_SEEDS {
            run_fuzz_session(seed, 64);
        }
    }

    #[test]
    fn test_fuzz_invariants_hold_at_contributor_scale() {
        // Longer run over the same slot pool: exercises repeated churn on every
        // username rather than a single pass.
        run_fuzz_session(0xc0ff_ee42, 256);
    }

    #[test]
    fn test_fuzz_failure_paths_leave_invariants_intact() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone())
                .unwrap();
        });

        let mut prng = Prng::new(0xfa11_ed01);

        for _ in 0..48 {
            let missing = FUZZ_MISSING[prng.below(FUZZ_MISSING.len() as u32) as usize];
            let name = username(&env, missing);
            let op = prng.below(3);

            env.as_contract(&contract_id, || {
                let result = match op {
                    0 => TrustBridgeContract::verify(env.clone(), name.clone()),
                    1 => TrustBridgeContract::revoke_verification(env.clone(), name.clone()),
                    _ => TrustBridgeContract::remove(env.clone(), admin.clone(), name.clone()),
                };

                assert_eq!(result, Err(ContractError::NotRegistered));

                // Rejected operations must not move any counter.
                let stats = TrustBridgeContract::get_stats(env.clone());
                assert_eq!(stats.total, 1);
                assert_eq!(stats.verified, 0);
            });
        }
    }

    // === Cost benchmarks

    /// Registry sizes probed by the export benchmark. The 100 case mirrors the
    /// contributor scale the registry is expected to reach.
    const BENCH_SIZES: [u32; 4] = [1, 10, 50, 100];

    fn bench_username(env: &Env, i: u32) -> String {
        String::from_str(env, &std::format!("contributor{:04}", i))
    }

    /// Populates `size` registrations, then meters a single admin export over
    /// that registry. Returns `(cpu_instructions, memory_bytes)`.
    fn measure_export(size: u32) -> (u64, u64) {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            for i in 0..size {
                TrustBridgeContract::register(env.clone(), bench_username(&env, i), user.clone())
                    .unwrap();
            }
        });

        // Reset after setup so only the export itself is metered. Unlimited
        // keeps tracking on while removing the ledger ceiling, which a 100-entry
        // export would otherwise trip during measurement.
        env.cost_estimate().budget().reset_unlimited();

        env.as_contract(&contract_id, || {
            let exported = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(exported.len(), size);
        });

        let budget = env.cost_estimate().budget();
        (budget.cpu_instruction_cost(), budget.memory_bytes_cost())
    }

    #[test]
    fn test_bench_export_cpu_cost() {
        std::println!("operation,size,cpu_instructions,memory_bytes");

        let mut previous_cpu = 0u64;
        let mut baseline: Option<(u32, u64)> = None;
        let mut largest: Option<(u32, u64)> = None;

        for size in BENCH_SIZES {
            let (cpu, mem) = measure_export(size);
            std::println!("get_all_registered,{},{},{}", size, cpu, mem);

            assert!(cpu > 0, "export at size {size} was not metered");
            // Cost is monotonic in registry size; a drop means the export
            // stopped visiting every record.
            assert!(
                cpu >= previous_cpu,
                "export CPU cost dropped at size {size}: {cpu} < {previous_cpu}"
            );

            previous_cpu = cpu;
            baseline.get_or_insert((size, cpu));
            largest = Some((size, cpu));
        }

        let (small_size, small_cpu) = baseline.unwrap();
        let (large_size, large_cpu) = largest.unwrap();

        // Export is a linear scan. Allow 3x headroom over the size ratio so
        // normal per-entry overhead passes while quadratic growth fails.
        let ceiling = small_cpu * ((large_size / small_size) as u64) * 3;
        assert!(
            large_cpu <= ceiling,
            "export CPU cost grew super-linearly: {large_cpu} at size {large_size} exceeds ceiling {ceiling}"
        );
    }

    #[test]
    fn test_bench_core_operation_cpu_cost() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        let report = |label: &str, cpu: u64, mem: u64| {
            std::println!("{},1,{},{}", label, cpu, mem);
            assert!(cpu > 0, "{label} was not metered");
        };

        env.cost_estimate().budget().reset_unlimited();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });
        let budget = env.cost_estimate().budget();
        report("register", budget.cpu_instruction_cost(), budget.memory_bytes_cost());

        env.cost_estimate().budget().reset_unlimited();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
        });
        let budget = env.cost_estimate().budget();
        report("get_address", budget.cpu_instruction_cost(), budget.memory_bytes_cost());

        env.cost_estimate().budget().reset_unlimited();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::get_stats(env.clone());
        });
        let budget = env.cost_estimate().budget();
        report("get_stats", budget.cpu_instruction_cost(), budget.memory_bytes_cost());
    }

    #[test]
    fn test_bench_failure_path_costs_less_than_success() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.cost_estimate().budget().reset_unlimited();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), username(&env, "missing"));
            assert_eq!(result, Err(ContractError::NotRegistered));
        });
        let rejected_cpu = env.cost_estimate().budget().cpu_instruction_cost();

        env.cost_estimate().budget().reset_unlimited();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });
        let accepted_cpu = env.cost_estimate().budget().cpu_instruction_cost();

        std::println!("verify_rejected,1,{},0", rejected_cpu);
        std::println!("verify_accepted,1,{},0", accepted_cpu);

        assert!(rejected_cpu > 0, "rejected call was not metered");
        // An early rejection must not do more work than the accepted path, or
        // a missing-username lookup becomes a cheap way to burn ledger budget.
        assert!(
            rejected_cpu < accepted_cpu,
            "rejected verify cost {rejected_cpu} is not below accepted cost {accepted_cpu}"
        );
    }

    #[test]
    fn test_fuzz_counters_never_underflow_on_empty_registry() {
        let env = Env::default();
        let (admin, _user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        let mut prng = Prng::new(0x0000_dead);

        for _ in 0..32 {
            let name = username(&env, FUZZ_USERNAMES[prng.below(FUZZ_USERNAMES.len() as u32) as usize]);

            env.as_contract(&contract_id, || {
                let result = TrustBridgeContract::remove(env.clone(), admin.clone(), name.clone());
                assert_eq!(result, Err(ContractError::NotRegistered));

                let stats = TrustBridgeContract::get_stats(env.clone());
                assert_eq!(stats.total, 0);
                assert_eq!(stats.verified, 0);
            });
        }
    }
}
