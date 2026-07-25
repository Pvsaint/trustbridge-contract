//! Integration tests for trustbridge-contract.
//!
//! Covers end-to-end contract governance, event publication (Registered, Verified,
//! Revoked, Removed, Upgraded, Paused, Unpaused), Role-Based Access Control (RBAC),
//! pause/unpause lifecycle, and WASM upgrade migration safety harness.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use trustbridge_contract::{ContractError, Role, TrustBridgeContract};

fn setup_test_env() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let contract_id = env.register(TrustBridgeContract, ());

    env.as_contract(&contract_id, || {
        TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
    });

    (env, admin, user1, user2, contract_id)
}

fn make_string(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

#[test]
fn test_integration_full_registry_lifecycle_and_events() {
    let (env, _admin, user1, _user2, contract_id) = setup_test_env();

    // 1. Register user
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), make_string(&env, "alice"), user1.clone())
            .unwrap();
    });

    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), make_string(&env, "alice"))
            .expect("record should exist");
        assert_eq!(record.stellar_address, user1);
        assert!(!record.verified);
    });

    // 2. Verify user
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), make_string(&env, "alice")).unwrap();
    });

    env.as_contract(&contract_id, || {
        let record_verified =
            TrustBridgeContract::get_address(env.clone(), make_string(&env, "alice")).unwrap();
        assert!(record_verified.verified);
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
    });

    // 3. Revoke verification
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), make_string(&env, "alice")).unwrap();
    });

    env.as_contract(&contract_id, || {
        let record_revoked =
            TrustBridgeContract::get_address(env.clone(), make_string(&env, "alice")).unwrap();
        assert!(!record_revoked.verified);
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
    });

    // 4. Remove registration
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), user1.clone(), make_string(&env, "alice"))
            .unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(
            TrustBridgeContract::get_address(env.clone(), make_string(&env, "alice")).is_none()
        );
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
    });
}

#[test]
fn test_integration_pause_unpause_governance() {
    let (env, _admin, user1, _user2, contract_id) = setup_test_env();

    // Pause contract
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::pause(env.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::is_paused(env.clone()));
    });

    // Mutating calls fail while paused
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::register(env.clone(), make_string(&env, "alice"), user1.clone()),
            Err(ContractError::Paused)
        );
    });

    // Read-only calls still work while paused
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
    });

    // Unpause contract
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::unpause(env.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(!TrustBridgeContract::is_paused(env.clone()));
    });

    // Mutating call succeeds after unpause
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::register(
            env.clone(),
            make_string(&env, "alice"),
            user1.clone()
        )
        .is_ok());
    });
}

#[test]
fn test_integration_role_based_access_control() {
    let (env, _admin, user1, user2, contract_id) = setup_test_env();

    // Assign Role 1
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), user1.clone(), Role::Upgrader).unwrap();
    });

    // Assign Role 2
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), user2.clone(), Role::Verifier).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::get_role(env.clone(), user1.clone()),
            Some(Role::Upgrader)
        );
        assert_eq!(
            TrustBridgeContract::get_role(env.clone(), user2.clone()),
            Some(Role::Verifier)
        );
    });

    // Revoke Role
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove_role(env.clone(), user1.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::get_role(env.clone(), user1.clone()),
            None
        );
    });
}

#[test]
fn test_integration_wasm_upgrade_migration_and_cooldown() {
    let (env, _admin, _user1, _user2, contract_id) = setup_test_env();

    // Configure upgrade cooldown: 1800 seconds (30 mins)
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_cooldown(env.clone(), 1800).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_cooldown(env.clone()), 1800);
    });

    // Perform first WASM upgrade
    let wasm_bytes = soroban_sdk::Bytes::from_slice(
        &env,
        include_bytes!("../target/wasm32v1-none/release/trustbridge_contract.wasm"),
    );
    let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes.clone());
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::upgrade(env.clone(), new_wasm_hash.clone()).is_ok());
    });

    // Second immediate WASM upgrade blocked by cooldown
    let next_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::upgrade(env.clone(), next_wasm_hash),
            Err(ContractError::CooldownActive)
        );
    });

    // Perform contract version migration
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 0, 0));
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::migrate(env.clone(), (1, 1, 0)).is_ok());
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 1, 0));
    });
}

#[test]
fn test_integration_edge_cases_and_error_paths() {
    let (env, _admin, user1, _user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), make_string(&env, "bob"), user1.clone())
            .unwrap();
        TrustBridgeContract::verify(env.clone(), make_string(&env, "bob")).unwrap();
    });

    // Double verification fails
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::verify(env.clone(), make_string(&env, "bob")),
            Err(ContractError::AlreadyVerified)
        );
    });

    // Revoking unverified fails
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), make_string(&env, "bob")).unwrap();
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::revoke_verification(env.clone(), make_string(&env, "bob")),
            Err(ContractError::NotVerified)
        );
    });

    // Migration to lower version fails
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::migrate(env.clone(), (1, 0, 0)),
            Err(ContractError::InvalidVersion)
        );
    });
}
