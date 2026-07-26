//! Integration tests for trustbridge-contract.
//!
//! Covers end-to-end contract governance, event publication (Registered,
//! Verified, Revoked, Removed, Upgraded, Paused, Unpaused), Role-Based Access
//! Control (RBAC), pause/unpause lifecycle, verifier role separation (Issue
//! #12), lookup after peer removal (Issue #52), not-initialized guards (Issue
//! #54), and verification attestation storage (Issue #16).

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

fn s(env: &Env, text: &str) -> String {
    String::from_str(env, text)
}

// ── Full lifecycle ────────────────────────────────────────────────────────────

#[test]
fn test_integration_full_registry_lifecycle_and_events() {
    let (env, admin, user1, _user2, contract_id) = setup_test_env();

    // Register
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), s(&env, "alice"))
            .expect("record should exist after register");
        assert_eq!(record.stellar_address, user1);
        assert!(!record.verified);
    });

    // Verify (Issue #12 — admin as caller)
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });

    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap();
        assert!(record.verified, "record must be verified after verify()");
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
    });

    // Revoke verification (Issue #12 — admin as caller)
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });

    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap();
        assert!(!record.verified, "record must be unverified after revoke");
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
    });

    // Remove
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), user1.clone(), s(&env, "alice")).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).is_none());
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
    });
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn test_integration_pause_unpause_governance() {
    let (env, _admin, user1, _user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::pause(env.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::is_paused(env.clone()));
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()),
            Err(ContractError::Paused)
        );
    });

    // Read-only still works while paused
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::unpause(env.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert!(!TrustBridgeContract::is_paused(env.clone()));
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).is_ok());
    });
}

// ── Role-based access control ─────────────────────────────────────────────────

#[test]
fn test_integration_role_based_access_control() {
    let (env, _admin, user1, user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), user1.clone(), Role::Upgrader).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), user2.clone(), Role::Verifier).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_role(env.clone(), user1.clone()), Some(Role::Upgrader));
        assert_eq!(TrustBridgeContract::get_role(env.clone(), user2.clone()), Some(Role::Verifier));
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove_role(env.clone(), user1.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_role(env.clone(), user1.clone()), None);
    });
}

// ── Issue #12: Verifier role separation ──────────────────────────────────────

#[test]
fn test_integration_verifier_role_separation() {
    let (env, _admin, user1, verifier, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "octocat"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), verifier.clone(), s(&env, "octocat")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "octocat")).unwrap().verified);
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), verifier.clone(), s(&env, "octocat")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "octocat")).unwrap().verified);
    });
}

#[test]
fn test_integration_no_role_cannot_verify() {
    let (env, _admin, user1, nobody, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "octocat"), user1.clone()).unwrap();
        let result = TrustBridgeContract::verify(env.clone(), nobody.clone(), s(&env, "octocat"));
        assert_eq!(result, Err(ContractError::NotAuthorized));
    });
}

// ── Issue #52: Lookup after peer removal ─────────────────────────────────────

#[test]
fn test_integration_lookup_after_peer_removal() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();
    let user3 = Address::generate(&env);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
        TrustBridgeContract::register(env.clone(), s(&env, "carol"), user3.clone()).unwrap();

        // Remove the first peer
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "alice")).unwrap();

        // bob and carol must still be accessible
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).is_none());
        assert_eq!(
            TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().stellar_address,
            user2
        );
        assert_eq!(
            TrustBridgeContract::get_address(env.clone(), s(&env, "carol")).unwrap().stellar_address,
            user3
        );
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 2);
    });
}

#[test]
fn test_integration_export_consistent_after_removal() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();
    let user3 = Address::generate(&env);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
        TrustBridgeContract::register(env.clone(), s(&env, "carol"), user3.clone()).unwrap();

        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "bob")).unwrap();

        let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
        assert_eq!(all.len(), 2, "export must skip removed entries");

        // The two remaining entries should be alice and carol
        let names: soroban_sdk::Vec<String> = {
            let mut v = soroban_sdk::Vec::new(&env);
            for i in 0..all.len() {
                v.push_back(all.get(i).unwrap().0);
            }
            v
        };
        assert!(names.contains(&s(&env, "alice")));
        assert!(names.contains(&s(&env, "carol")));
    });
}

// ── Issue #54: Not-initialized guard coverage ─────────────────────────────────

#[test]
fn test_integration_not_initialized_guards() {
    let env = Env::default();
    let contract_id = env.register(TrustBridgeContract, ());
    let addr = Address::generate(&env);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::register(env.clone(), s(&env, "alice"), addr.clone()),
            Err(ContractError::NotInitialized),
            "register before init"
        );
        assert_eq!(
            TrustBridgeContract::remove(env.clone(), addr.clone(), s(&env, "alice")),
            Err(ContractError::NotInitialized),
            "remove before init"
        );
        assert_eq!(
            TrustBridgeContract::verify(env.clone(), addr.clone(), s(&env, "alice")),
            Err(ContractError::NotInitialized),
            "verify before init"
        );
        assert_eq!(
            TrustBridgeContract::revoke_verification(env.clone(), addr.clone(), s(&env, "alice")),
            Err(ContractError::NotInitialized),
            "revoke_verification before init"
        );
        assert_eq!(
            TrustBridgeContract::pause(env.clone()),
            Err(ContractError::NotInitialized),
            "pause before init"
        );
        assert_eq!(
            TrustBridgeContract::unpause(env.clone()),
            Err(ContractError::NotInitialized),
            "unpause before init"
        );
        assert_eq!(
            TrustBridgeContract::set_role(env.clone(), addr.clone(), Role::Verifier),
            Err(ContractError::NotInitialized),
            "set_role before init"
        );
        assert_eq!(
            TrustBridgeContract::remove_role(env.clone(), addr.clone()),
            Err(ContractError::NotInitialized),
            "remove_role before init"
        );
        assert_eq!(
            TrustBridgeContract::set_cooldown(env.clone(), 100),
            Err(ContractError::NotInitialized),
            "set_cooldown before init"
        );
        assert_eq!(
            TrustBridgeContract::get_all_registered(env.clone()),
            Err(ContractError::NotInitialized),
            "get_all_registered before init"
        );
        assert_eq!(
            TrustBridgeContract::migrate(env.clone(), (2, 0, 0)),
            Err(ContractError::NotInitialized),
            "migrate before init"
        );
    });
}

// ── Issue #16: Verification attestation storage ───────────────────────────────

#[test]
fn test_integration_verification_attestation_storage() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap().verified);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().verified);
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap().verified);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().verified,
            "bob's verification status must be unaffected by alice's verification");
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "bob")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 2);
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap().verified);
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().verified,
            "bob must remain verified after alice's revocation");
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "bob")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 0);
    });
}

#[test]
fn test_integration_attestation_preserved_on_same_address_reregister() {
    let (env, admin, user1, _user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap();
        assert!(record.verified, "same-address re-register must preserve attestation");
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
    });
}

#[test]
fn test_integration_attestation_cleared_on_address_change() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user2.clone()).unwrap();
    });
    env.as_contract(&contract_id, || {
        let record = TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap();
        assert!(!record.verified, "address change must clear attestation");
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
    });
}

// ── Version migration ─────────────────────────────────────────────────────────

#[test]
fn test_integration_version_migration() {
    let (env, _admin, _user1, _user2, contract_id) = setup_test_env();

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 0, 0));
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::migrate(env.clone(), (1, 0, 0)),
            Err(ContractError::InvalidVersion)
        );
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::migrate(env.clone(), (1, 1, 0)).unwrap();
    });

    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 1, 0));
    });
}

// ── WASM upgrade + cooldown (requires pre-built WASM) ─────────────────────────

#[test]
#[cfg(feature = "wasm-test")]
fn test_integration_wasm_upgrade_cooldown() {
    let (env, _admin, _user1, _user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_cooldown(env.clone(), 1800).unwrap();
        assert_eq!(TrustBridgeContract::get_cooldown(env.clone()), 1800);
    });

    let wasm_bytes = soroban_sdk::Bytes::from_slice(
        &env,
        include_bytes!("../target/wasm32v1-none/release/trustbridge_contract.wasm"),
    );
    let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes.clone());
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::upgrade(env.clone(), new_wasm_hash).is_ok());
    });

    let next_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::upgrade(env.clone(), next_wasm_hash),
            Err(ContractError::CooldownActive)
        );
    });
}

// ── Issue #54: Additional not-initialized guard tests (integration) ───────────

/// get_registered_page must return NotInitialized before init (Issue #54).
#[test]
fn test_integration_get_registered_page_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(TrustBridgeContract, ());
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::get_registered_page(env.clone(), 0, 10),
            Err(ContractError::NotInitialized),
            "get_registered_page before init"
        );
    });
}

/// get_registered_paginated must return NotInitialized before init (Issue #54).
#[test]
fn test_integration_get_registered_paginated_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(TrustBridgeContract, ());
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::get_registered_paginated(env.clone(), 0, 10),
            Err(ContractError::NotInitialized),
            "get_registered_paginated before init"
        );
    });
}

/// get_public_paginated must return NotInitialized before init (Issue #54).
#[test]
fn test_integration_get_public_paginated_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(TrustBridgeContract, ());
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::get_public_paginated(env.clone(), 0, 10),
            Err(ContractError::NotInitialized),
            "get_public_paginated before init"
        );
    });
}

/// Once initialized, previously failing calls must succeed (Issue #54).
#[test]
fn test_integration_guards_lifted_after_initialization() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register(TrustBridgeContract, ());

    // All mutating calls fail before init
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::register(env.clone(), s(&env, "alice"), user.clone()),
            Err(ContractError::NotInitialized)
        );
    });

    // Initialize
    env.as_contract(&contract_id, || {
        TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
    });

    // Same calls must now pass
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::register(env.clone(), s(&env, "alice"), user.clone()).is_ok());
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        let page = TrustBridgeContract::get_registered_paginated(env.clone(), 0, 10).unwrap();
        assert_eq!(page.records.len(), 1);
    });
}

// ── Issue #52: Additional lookup-after-peer-removal (integration) ─────────────

/// Paginated admin export is consistent after multiple removals (Issue #52).
#[test]
fn test_integration_paginated_export_after_multiple_removals() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();
    let user3 = Address::generate(&env);
    let user4 = Address::generate(&env);

    for (name, addr) in [
        (s(&env, "alice"), user1.clone()),
        (s(&env, "bob"),   user2.clone()),
        (s(&env, "carol"), user3.clone()),
        (s(&env, "dave"),  user4.clone()),
    ] {
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), name, addr).unwrap();
        });
    }
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "carol")).unwrap();
    });

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
        assert_eq!(all.len(), 2, "only bob and dave must remain");
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        let page = TrustBridgeContract::get_registered_paginated(env.clone(), 0, 10).unwrap();
        assert_eq!(page.records.len(), 2);
        assert_eq!(page.total, 2);
        assert!(!page.has_more);
    });
}

/// Public paginated endpoint is consistent after peer removal (Issue #52).
#[test]
fn test_integration_public_paginated_after_peer_removal() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();
    let user3 = Address::generate(&env);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "carol"), user3.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "bob")).unwrap();
    });
    env.as_contract(&contract_id, || {
        let page = TrustBridgeContract::get_public_paginated(env.clone(), 0, 10).unwrap();
        assert_eq!(page.records.len(), 2, "public paginated must skip removed bob");
        let names: soroban_sdk::Vec<String> = {
            let mut v = soroban_sdk::Vec::new(&env);
            for i in 0..page.records.len() {
                v.push_back(page.records.get(i).unwrap().0);
            }
            v
        };
        assert!(names.contains(&s(&env, "alice")));
        assert!(names.contains(&s(&env, "carol")));
    });
}

// ── Issue #12: Additional verifier role separation (integration) ──────────────

/// Revoking Verifier role prevents the former holder from verifying (Issue #12).
#[test]
fn test_integration_revoked_verifier_cannot_verify() {
    let (env, _admin, user1, verifier, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), verifier.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), verifier.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove_role(env.clone(), verifier.clone()).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_role(env.clone(), verifier.clone()), None);
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        let result = TrustBridgeContract::verify(env.clone(), verifier.clone(), s(&env, "alice"));
        assert_eq!(result, Err(ContractError::NotAuthorized));
    });
}

/// Upgrader role cannot verify or revoke verification (Issue #12).
#[test]
fn test_integration_upgrader_cannot_verify_or_revoke() {
    let (env, admin, user1, upgrader, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::set_role(env.clone(), upgrader.clone(), Role::Upgrader).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::verify(env.clone(), upgrader.clone(), s(&env, "alice")),
            Err(ContractError::NotAuthorized),
            "Upgrader must not verify"
        );
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        assert_eq!(
            TrustBridgeContract::revoke_verification(env.clone(), upgrader.clone(), s(&env, "alice")),
            Err(ContractError::NotAuthorized),
            "Upgrader must not revoke verification"
        );
    });
}

// ── Issue #16: Additional verification attestation storage (integration) ──────

/// ContributorRecord fields are durably persisted and independently isolated
/// per username (Issue #16).
#[test]
fn test_integration_attestation_record_fields_isolated() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();
    let user3 = Address::generate(&env);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "carol"), user3.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "carol")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap().verified);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().verified,
            "bob must remain unverified");
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "carol")).unwrap().verified);
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 2);
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), s(&env, "carol")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert!(TrustBridgeContract::get_address(env.clone(), s(&env, "alice")).unwrap().verified,
            "alice must remain verified after carol revocation");
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "bob")).unwrap().verified);
        assert!(!TrustBridgeContract::get_address(env.clone(), s(&env, "carol")).unwrap().verified);
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
    });
}

/// Verification count never goes negative on repeated revocations (Issue #16).
#[test]
fn test_integration_vcount_never_underflows() {
    let (env, admin, user1, _user2, contract_id) = setup_test_env();

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    env.as_contract(&contract_id, || {
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        let result = TrustBridgeContract::revoke_verification(
            env.clone(),
            admin.clone(),
            s(&env, "alice"),
        );
        assert_eq!(result, Err(ContractError::NotVerified));
        assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0,
            "vcount must not underflow below zero");
    });
}

/// get_stats().verified matches get_verified_count() at every step (Issue #16).
#[test]
fn test_integration_stats_verified_matches_verified_count() {
    let (env, admin, user1, user2, contract_id) = setup_test_env();

    let check = |env: &Env, cid: &Address| {
        env.as_contract(cid, || {
            assert_eq!(
                TrustBridgeContract::get_stats(env.clone()).verified,
                TrustBridgeContract::get_verified_count(env.clone()),
                "get_stats().verified must equal get_verified_count()"
            );
        });
    };

    check(&env, &contract_id);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "alice"), user1.clone()).unwrap();
    });
    check(&env, &contract_id);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    check(&env, &contract_id);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::register(env.clone(), s(&env, "bob"), user2.clone()).unwrap();
    });
    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::verify(env.clone(), admin.clone(), s(&env, "bob")).unwrap();
    });
    check(&env, &contract_id);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), s(&env, "alice")).unwrap();
    });
    check(&env, &contract_id);

    env.mock_all_auths();
    env.as_contract(&contract_id, || {
        TrustBridgeContract::remove(env.clone(), admin.clone(), s(&env, "bob")).unwrap();
    });
    check(&env, &contract_id);
}
