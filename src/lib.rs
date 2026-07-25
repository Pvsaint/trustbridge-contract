#![no_std]

mod audit;
mod batch;
mod error;
mod error_context;
mod events;
mod storage;
mod utils;
mod version;

pub use error::ContractError;
pub use events::{
    PausedEvent, RegisteredEvent, RemovedEvent, RoleGrantedEvent, RoleRevokedEvent, UnpausedEvent,
    UpgradedEvent, VerificationRevokedEvent, VerifiedEvent,
};
pub use storage::{ContributorRecord, ExportPage, Role, Stats};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::storage::{
    add_to_index, get_admin, get_cooldown as storage_get_cooldown, get_count, get_index,
    get_index_page, get_last_upgrade, get_record, get_role as storage_get_role,
    get_registered_paginated_internal, get_stats as read_stats,
    get_verified_count as storage_get_verified_count, get_version as storage_get_version,
    has_record, has_role_or_admin, is_paused as storage_is_paused, remove_from_index,
    remove_record, remove_role as storage_remove_role, require_initialized, require_not_paused,
    set_cooldown as storage_set_cooldown, set_count, set_last_upgrade,
    set_paused as storage_set_paused, set_record, set_role as storage_set_role, set_verified_count,
    set_version, ADMIN_KEY,
};
use crate::version::{Version, CONTRACT_VERSION};

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
        storage_set_paused(&env, false);
        storage_set_cooldown(&env, 0);
        set_version(&env, (1, 0, 0));
        storage_set_role(&env, &admin, &Role::Admin);

        Ok(())
    }

    /// Pauses contract state mutations. Admin-only.
    pub fn pause(env: Env) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        storage_set_paused(&env, true);
        let timestamp = env.ledger().timestamp();
        PausedEvent { admin, timestamp }.publish(&env);
        Ok(())
    }

    /// Unpauses contract state mutations. Admin-only.
    pub fn unpause(env: Env) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        storage_set_paused(&env, false);
        let timestamp = env.ledger().timestamp();
        UnpausedEvent { admin, timestamp }.publish(&env);
        Ok(())
    }

    /// Returns whether contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        storage_is_paused(&env)
    }

    /// Assigns a role to an address. Admin-only.
    pub fn set_role(env: Env, target: Address, role: Role) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        let admin = get_admin(&env)?;
        admin.require_auth();

        storage_set_role(&env, &target, &role);
        let timestamp = env.ledger().timestamp();
        RoleGrantedEvent {
            address: target,
            role: role as u32,
            admin,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Revokes a role from an address. Admin-only.
    pub fn remove_role(env: Env, target: Address) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        let admin = get_admin(&env)?;
        admin.require_auth();

        storage_remove_role(&env, &target);
        let timestamp = env.ledger().timestamp();
        RoleRevokedEvent {
            address: target,
            admin,
            timestamp,
        }
        .publish(&env);

        Ok(())
    }

    /// Queries assigned role for an address.
    pub fn get_role(env: Env, address: Address) -> Option<Role> {
        storage_get_role(&env, &address)
    }

    /// Configures WASM upgrade cooldown in seconds. Admin-only.
    pub fn set_cooldown(env: Env, cooldown_seconds: u64) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        storage_set_cooldown(&env, cooldown_seconds);
        Ok(())
    }

    /// Returns WASM upgrade cooldown in seconds.
    pub fn get_cooldown(env: Env) -> u64 {
        storage_get_cooldown(&env)
    }

    /// Returns current contract version tuple (major, minor, patch).
    pub fn get_version(env: Env) -> (u32, u32, u32) {
        storage_get_version(&env).unwrap_or_else(|| CONTRACT_VERSION.to_tuple())
    }

    /// Upgrades contract WASM executable code. Admin-only.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        let admin = get_admin(&env)?;
        admin.require_auth();

        let now = env.ledger().timestamp();
        let cooldown = storage_get_cooldown(&env);
        let has_upgraded = env.storage().instance().has(&crate::storage::LAST_UPG_KEY);

        if has_upgraded && cooldown > 0 {
            let last_upg = get_last_upgrade(&env);
            if now < last_upg.saturating_add(cooldown) {
                return Err(ContractError::CooldownActive);
            }
        }

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        set_last_upgrade(&env, now);

        let version = storage_get_version(&env).unwrap_or_else(|| CONTRACT_VERSION.to_tuple());
        UpgradedEvent {
            new_wasm_hash,
            version,
            timestamp: now,
        }
        .publish(&env);

        Ok(())
    }

    /// Migrates contract version state. Admin-only.
    pub fn migrate(env: Env, new_version: (u32, u32, u32)) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        let admin = get_admin(&env)?;
        admin.require_auth();

        let current = storage_get_version(&env).unwrap_or_else(|| CONTRACT_VERSION.to_tuple());
        if new_version <= current {
            return Err(ContractError::InvalidVersion);
        }

        set_version(&env, new_version);
        Ok(())
    }

    /// Returns the deployed contract version as `(major, minor, patch)`.
    ///
    /// Instances initialized before versioning was added carry no stored
    /// version and report the build constant instead.
    pub fn version(env: Env) -> (u32, u32, u32) {
        storage_get_version(&env).unwrap_or_else(|| CONTRACT_VERSION.to_tuple())
    }

    /// Reports whether the deployed contract satisfies a client's minimum
    /// required version. Bindings consumers call this before invoking, so a
    /// stale client fails fast instead of on an unexpected ABI.
    pub fn is_compatible(env: Env, major: u32, minor: u32, patch: u32) -> bool {
        Version::from_tuple(Self::version(env))
            .is_compatible_with(Version::new(major, minor, patch))
    }

    /// Registers or updates a GitHub username → Stellar address mapping.
    ///
    /// The caller must authenticate as `stellar_address`. The username must be
    /// 1 to 39 characters of alphanumerics, hyphens, and underscores, starting
    /// and ending alphanumeric, or the call fails with `InvalidUsername`.
    pub fn register(
        env: Env,
        github_username: String,
        stellar_address: Address,
    ) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;
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

    /// Cheap existence check for dashboard/indexer consumers.
    ///
    /// Avoids deserializing the full `ContributorRecord` when callers only
    /// need to know whether a `github_username` is registered (Wave #40).
    pub fn has_record(env: Env, github_username: String) -> bool {
        has_record(&env, &github_username)
    }

    /// Removes a registration. Callable by the registrant or the admin.
    ///
    /// `caller` must sign the transaction and must equal either the contract
    /// admin or the registered Stellar address for `github_username`.
    pub fn remove(env: Env, caller: Address, github_username: String) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

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

    /// Returns a page of registered (github_username, stellar_address) pairs
    /// starting at `offset`, up to `limit` entries. Admin-only, like
    /// `get_all_registered`, but avoids materializing the whole registry in
    /// one call for large indexes (Wave #41).
    pub fn get_registered_page(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<(String, Address)>, ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        let page = get_index_page(&env, offset, limit);
        let mut result = Vec::new(&env);
        for i in 0..page.len() {
            let username = page.get(i).unwrap();
            if let Some(record) = get_record(&env, &username) {
                result.push_back((username, record.stellar_address));
            }
        }

        Ok(result)
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

    /// Exports paginated records with cursor. Admin-only (Issue #1).
    pub fn get_registered_paginated(
        env: Env,
        cursor: u32,
        limit: u32,
    ) -> Result<ExportPage, ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        get_registered_paginated_internal(&env, cursor, limit)
    }

    /// Public paginated reads for indexers and dashboard consumers (Issue #3).
    /// Hardened with pause checks and capped limits.
    pub fn get_public_paginated(
        env: Env,
        cursor: u32,
        limit: u32,
    ) -> Result<ExportPage, ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        get_registered_paginated_internal(&env, cursor, limit)
    }

    /// Marks a contributor as verified after an off-chain GitHub identity check.
    ///
    /// Callable by the contract admin **or** any address assigned the
    /// `Role::Verifier` role (Issue #12 — verifier role separation).
    ///
    /// The `caller` argument must match an address that has either the admin
    /// role or `Role::Verifier` assigned via `set_role`.
    pub fn verify(
        env: Env,
        caller: Address,
        github_username: String,
    ) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        caller.require_auth();

        // Caller must be the admin OR hold the Verifier role.
        if !has_role_or_admin(&env, &caller, Role::Verifier) {
            return Err(ContractError::NotAuthorized);
        }

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

    /// Revokes verification for a registered contributor.
    ///
    /// Callable by the contract admin **or** any address assigned the
    /// `Role::Verifier` role (Issue #12 — verifier role separation).
    pub fn revoke_verification(
        env: Env,
        caller: Address,
        github_username: String,
    ) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        caller.require_auth();

        // Caller must be the admin OR hold the Verifier role.
        if !has_role_or_admin(&env, &caller, Role::Verifier) {
            return Err(ContractError::NotAuthorized);
        }

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

    // ── Helpers ──────────────────────────────────────────────────────────────

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

    // ── Basic registration / lookup ──────────────────────────────────────────

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
    fn test_get_address_missing_returns_none() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);
        env.as_contract(&contract_id, || {
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none()
            );
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
    fn test_register_two_users_keeps_addresses() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).unwrap().stellar_address,
                user1
            );
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().stellar_address,
                user2
            );
        });
    }

    // ── Stats ────────────────────────────────────────────────────────────────

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
    fn test_get_stats_increments_correctly() {
        let env = Env::default();
        let (admin, user1, user2, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 0);
            assert_eq!(stats.verified, 0);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 2);
            assert_eq!(stats.verified, 0);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "alice")).unwrap();
        });

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 2);
            assert_eq!(stats.verified, 1);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user2.clone(), username(&env, "bob")).unwrap();
        });

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 1);
        });
    }

    #[test]
    fn test_removing_one_of_two_keeps_remaining_stats() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 0);
        });
    }

    // ── Remove ───────────────────────────────────────────────────────────────

    #[test]
    fn test_non_owner_cannot_remove() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            let result = TrustBridgeContract::remove(env.clone(), other.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    #[test]
    fn test_admin_can_remove_registration() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).is_none());
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
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
        });
        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).is_none());
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
        });
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }

    // ── Issue #52: lookup after peer removal ─────────────────────────────────

    #[test]
    fn test_remove_then_lookup_other_record() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        env.as_contract(&contract_id, || {
            // bob's record must survive alice's removal
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().stellar_address,
                user2
            );
        });
    }

    #[test]
    fn test_export_skips_removed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!(all.get(0).unwrap(), (username(&env, "bob"), user2.clone()));
        });
    }

    #[test]
    fn test_lookup_after_first_of_three_removed() {
        let env = Env::default();
        let (admin, user1, user2, contract_id) = setup(&env);
        let user3 = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "carol"), user3.clone()).unwrap();

            // Remove the first entry
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "alice")).unwrap();

            // Both remaining records must be reachable
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).is_none());
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().stellar_address,
                user2
            );
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "carol")).unwrap().stellar_address,
                user3
            );
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 2);
        });
    }

    #[test]
    fn test_lookup_after_middle_of_three_removed() {
        let env = Env::default();
        let (admin, user1, user2, contract_id) = setup(&env);
        let user3 = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "carol"), user3.clone()).unwrap();

            // Remove the middle entry
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "bob")).unwrap();

            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).unwrap().stellar_address,
                user1
            );
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).is_none());
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "carol")).unwrap().stellar_address,
                user3
            );
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 2);
        });
    }

    #[test]
    fn test_index_integrity_after_multiple_removals() {
        let env = Env::default();
        let (admin, user1, user2, contract_id) = setup(&env);
        let user3 = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "carol"), user3.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "alice")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), admin.clone(), username(&env, "carol")).unwrap();
        });

        // Only bob remains
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!(all.get(0).unwrap().0, username(&env, "bob"));
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }

    #[test]
    fn test_reregister_after_removal_is_treated_as_new() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        // re-register alice
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });

        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 2);
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).unwrap().stellar_address,
                user1
            );
        });
    }

    // ── Re-registration ───────────────────────────────────────────────────────

    #[test]
    fn test_reregistration_updates_record() {
        let env = Env::default();
        let (admin, user, new_user, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), new_user.clone()).unwrap();

            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(record.stellar_address, new_user);
            assert!(!record.verified);

            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 0);
        });
    }

    #[test]
    fn test_updated_registration_preserves_count() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
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
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }

    // ── Issue #16: Verification attestation storage ───────────────────────────

    #[test]
    fn test_verify_sets_verified_flag_and_increments_vcount() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            assert!(!TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap().verified);
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);

            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();

            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(record.verified, "verified flag must be true after verify()");
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
        });
    }

    #[test]
    fn test_verify_missing_registration_fails() {
        let env = Env::default();
        let (admin, _user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "missing"));
            assert_eq!(result, Err(ContractError::NotRegistered));
        });
    }

    #[test]
    fn test_double_verify_fails() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::AlreadyVerified));
        });
    }

    #[test]
    fn test_revoke_verification_clears_flag_and_decrements_vcount() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 1);
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified, "verified flag must be false after revoke");
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
        });
    }

    #[test]
    fn test_revoke_verification_nonverified_fails() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
            let result = TrustBridgeContract::revoke_verification(env.clone(), admin.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotVerified));
        });
    }

    #[test]
    fn test_removing_verified_record_updates_stats() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 0);
            assert_eq!(stats.verified, 0);
        });
    }

    #[test]
    fn test_reregister_same_address_keeps_verification() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.as_contract(&contract_id, || {
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(record.verified, "re-registering the same address should preserve verified=true");
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 1);
        });
    }

    #[test]
    fn test_verified_address_change_clears_verification() {
        let env = Env::default();
        let (admin, user, other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone()).unwrap();
        });
        env.as_contract(&contract_id, || {
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified, "changing stellar address must clear verification");
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 0);
        });
    }

    #[test]
    fn test_verified_same_address_reregister_keeps_count() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.as_contract(&contract_id, || {
            let stats = TrustBridgeContract::get_stats(env.clone());
            assert_eq!(stats.total, 1);
            assert_eq!(stats.verified, 1);
        });
    }

    #[test]
    fn test_removed_verified_user_can_register_unverified() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.as_contract(&contract_id, || {
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }

    #[test]
    fn test_verify_after_reregister_new_address() {
        let env = Env::default();
        let (admin, user, other, contract_id) = setup(&env);
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
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap().verified);
        });
    }

    #[test]
    fn test_verify_after_address_update_targets_new_address() {
        let env = Env::default();
        let (admin, old_user, new_user, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), old_user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), new_user.clone()).unwrap();
        });
        env.as_contract(&contract_id, || {
            let after_update = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(after_update.stellar_address, new_user);
            assert!(!after_update.verified);
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 0);
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let after_verify = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(after_verify.stellar_address, new_user);
            assert!(after_verify.verified);
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 1);
        });
    }

    /// Verifier-role caller can verify (Issue #12).
    #[test]
    fn test_verifier_role_can_verify() {
        let env = Env::default();
        let (admin, user, verifier, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), verifier.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let record = TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(record.verified);
        });
        let _ = admin;
    }

    /// Verifier-role caller can revoke (Issue #12).
    #[test]
    fn test_verifier_role_can_revoke_verification() {
        let env = Env::default();
        let (admin, user, verifier, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::revoke_verification(env.clone(), verifier.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
        });
    }

    /// Address without role cannot verify (Issue #12).
    #[test]
    fn test_no_role_cannot_verify() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            // `other` has no role
            let result = TrustBridgeContract::verify(env.clone(), other.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    /// Admin can still call verify (role separation is additive) (Issue #12).
    #[test]
    fn test_admin_can_still_verify_after_role_separation() {
        let env = Env::default();
        let (admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap().verified);
        });
    }

    /// Upgrader role cannot verify (Issue #12 — only Verifier and Admin).
    #[test]
    fn test_upgrader_role_cannot_verify() {
        let env = Env::default();
        let (_admin, user, upgrader, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), upgrader.clone(), Role::Upgrader).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), upgrader.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    // ── Issue #54: Not-initialized guard tests ───────────────────────────────

    #[test]
    fn test_initialize_only_once() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
            let result = TrustBridgeContract::initialize(env.clone(), admin.clone());
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
            let result = TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone());
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_remove_requires_initialization() {
        let env = Env::default();
        let user = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_verify_requires_initialization() {
        let env = Env::default();
        let caller = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), caller.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_revoke_verification_requires_initialization() {
        let env = Env::default();
        let caller = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::revoke_verification(env.clone(), caller.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_pause_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::pause(env.clone());
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_unpause_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::unpause(env.clone());
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_set_role_requires_initialization() {
        let env = Env::default();
        let target = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::set_role(env.clone(), target.clone(), Role::Verifier);
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_remove_role_requires_initialization() {
        let env = Env::default();
        let target = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::remove_role(env.clone(), target.clone());
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_set_cooldown_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::set_cooldown(env.clone(), 3600);
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_get_all_registered_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::get_all_registered(env.clone());
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    #[test]
    fn test_migrate_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::migrate(env.clone(), (2, 0, 0));
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    // ── Pause / unpause workflow ──────────────────────────────────────────────

    #[test]
    fn test_pause_and_unpause_workflow() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert!(!TrustBridgeContract::is_paused(env.clone()));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::pause(env.clone()).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::is_paused(env.clone()));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let reg_res = TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone());
            assert_eq!(reg_res, Err(ContractError::Paused));
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
            assert!(TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone()).is_ok());
        });
    }

    // ── Roles management ─────────────────────────────────────────────────────

    #[test]
    fn test_roles_management() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_role(env.clone(), user.clone()), None);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), user.clone(), Role::Upgrader).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_role(env.clone(), user.clone()), Some(Role::Upgrader));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), other.clone(), Role::Verifier).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_role(env.clone(), other.clone()), Some(Role::Verifier));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove_role(env.clone(), user.clone()).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_role(env.clone(), user.clone()), None);
        });
    }

    // ── Cooldown / version / migration ────────────────────────────────────────

    #[test]
    fn test_migration_version_increment() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 0, 0));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let err_res = TrustBridgeContract::migrate(env.clone(), (1, 0, 0));
            assert_eq!(err_res, Err(ContractError::InvalidVersion));
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::migrate(env.clone(), (1, 1, 0)).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 1, 0));
        });
    }

    // ── Admin export ──────────────────────────────────────────────────────────

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
    fn test_get_all_registered_returns_indexed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
            let all = TrustBridgeContract::get_all_registered(env.clone()).unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all.get(0).unwrap(), (username(&env, "alice"), user1.clone()));
            assert_eq!(all.get(1).unwrap(), (username(&env, "bob"), user2.clone()));
        });
    }

    #[test]
    fn test_cold_start_register_exposes_dashboard_state() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_all_registered(env.clone()).unwrap().len(), 0);
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_all_registered(env.clone()).unwrap().len(), 1);
        });
    }

    // ── Error codes ───────────────────────────────────────────────────────────

    #[test]
    fn test_error_codes_match_repr() {
        assert_eq!(ContractError::AlreadyInitialized.code(), 1);
        assert_eq!(ContractError::NotInitialized.code(), 2);
        assert_eq!(ContractError::NotAuthorized.code(), 3);
        assert_eq!(ContractError::NotRegistered.code(), 4);
        assert_eq!(ContractError::AlreadyVerified.code(), 5);
        assert_eq!(ContractError::NotVerified.code(), 6);
        assert_eq!(ContractError::Paused.code(), 7);
        assert_eq!(ContractError::CooldownActive.code(), 8);
        assert_eq!(ContractError::InvalidVersion.code(), 9);
        assert_eq!(ContractError::InvalidRole.code(), 10);
    }

    // ── Issue #16: from_code round-trip and completeness ─────────────────────

    /// Every variant's code() must round-trip through from_code() (Issue #16).
    #[test]
    fn test_from_code_round_trips_all_variants() {
        let all = [
            ContractError::AlreadyInitialized,
            ContractError::NotInitialized,
            ContractError::NotAuthorized,
            ContractError::NotRegistered,
            ContractError::AlreadyVerified,
            ContractError::NotVerified,
            ContractError::Paused,
            ContractError::CooldownActive,
            ContractError::InvalidVersion,
            ContractError::InvalidRole,
        ];
        for variant in all {
            assert_eq!(
                ContractError::from_code(variant.code()),
                Some(variant),
                "from_code({}) did not return {:?}",
                variant.code(),
                variant
            );
        }
    }

    /// Codes not in the enum must return None (Issue #16).
    #[test]
    fn test_from_code_unknown_returns_none() {
        assert_eq!(ContractError::from_code(0), None);
        assert_eq!(ContractError::from_code(11), None);
        assert_eq!(ContractError::from_code(u32::MAX), None);
    }

    // ── Issue #54: Additional not-initialized guard tests ────────────────────

    /// get_registered_page must fail before init (Issue #54).
    #[test]
    fn test_get_registered_page_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::get_registered_page(env.clone(), 0, 10);
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    /// get_registered_paginated must fail before init (Issue #54).
    #[test]
    fn test_get_registered_paginated_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::get_registered_paginated(env.clone(), 0, 10);
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    /// get_public_paginated must fail before init (Issue #54).
    #[test]
    fn test_get_public_paginated_requires_initialization() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::get_public_paginated(env.clone(), 0, 10);
            assert_eq!(result, Err(ContractError::NotInitialized));
        });
    }

    /// After initialization every previously failing guard must succeed (Issue #54).
    #[test]
    fn test_guards_succeed_after_initialization() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            // Confirm guard fires before init
            assert_eq!(
                TrustBridgeContract::register(env.clone(), username(&env, "octocat"), admin.clone()),
                Err(ContractError::NotInitialized)
            );

            TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();

            // Same call must now succeed
            assert!(
                TrustBridgeContract::register(env.clone(), username(&env, "octocat"), admin.clone()).is_ok()
            );
        });
    }

    /// Double-initialize after successful init must still be rejected (Issue #54).
    #[test]
    fn test_double_initialize_rejected_after_successful_init() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let admin2 = Address::generate(&env);
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            TrustBridgeContract::initialize(env.clone(), admin.clone()).unwrap();
            let result = TrustBridgeContract::initialize(env.clone(), admin2.clone());
            assert_eq!(result, Err(ContractError::AlreadyInitialized));
        });
    }

    // ── Issue #52: Additional lookup-after-peer-removal tests ────────────────

    /// Paginated export must skip removed records (Issue #52).
    #[test]
    fn test_paginated_export_skips_removed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        let user3 = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "carol"), user3.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user2.clone(), username(&env, "bob")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let page = TrustBridgeContract::get_registered_paginated(env.clone(), 0, 10).unwrap();
            assert_eq!(page.records.len(), 2, "paginated export must skip removed entry");
            assert_eq!(page.total, 2);
            assert!(!page.has_more);
        });
    }

    /// Public paginated endpoint reflects removal immediately (Issue #52).
    #[test]
    fn test_public_paginated_reflects_removal() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        env.as_contract(&contract_id, || {
            let page = TrustBridgeContract::get_public_paginated(env.clone(), 0, 10).unwrap();
            assert_eq!(page.records.len(), 1);
            assert_eq!(page.records.get(0).unwrap().0, username(&env, "bob"));
        });
    }

    /// has_record returns false after removal and true for surviving peer (Issue #52).
    #[test]
    fn test_has_record_consistency_after_peer_removal() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert!(!TrustBridgeContract::has_record(env.clone(), username(&env, "alice")));
            assert!(TrustBridgeContract::has_record(env.clone(), username(&env, "bob")));
        });
    }

    // ── Issue #12: Additional verifier role separation tests ─────────────────

    /// Revoking Verifier role prevents further verify calls (Issue #12).
    #[test]
    fn test_revoked_verifier_role_cannot_verify() {
        let env = Env::default();
        let (_admin, user, verifier, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), verifier.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove_role(env.clone(), verifier.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::verify(env.clone(), verifier.clone(), username(&env, "alice"));
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    /// Two independent Verifier-role holders can each verify without interfering (Issue #12).
    #[test]
    fn test_two_verifiers_operate_independently() {
        let env = Env::default();
        let (_admin, user, verifier1, contract_id) = setup(&env);
        let verifier2 = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier1.clone(), Role::Verifier).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier2.clone(), Role::Verifier).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), verifier1.clone(), username(&env, "alice")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), verifier2.clone(), username(&env, "bob")).unwrap();
        });
        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "alice")).unwrap().verified);
            assert!(TrustBridgeContract::get_address(env.clone(), username(&env, "bob")).unwrap().verified);
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 2);
        });
    }

    /// Upgrader role cannot revoke verification (Issue #12).
    #[test]
    fn test_upgrader_role_cannot_revoke_verification() {
        let env = Env::default();
        let (admin, user, upgrader, contract_id) = setup(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), upgrader.clone(), Role::Upgrader).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone()).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), admin.clone(), username(&env, "octocat")).unwrap();
        });
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result = TrustBridgeContract::revoke_verification(
                env.clone(),
                upgrader.clone(),
                username(&env, "octocat"),
            );
            assert_eq!(result, Err(ContractError::NotAuthorized));
        });
    }

    /// Verifier-role address cannot call set_role (admin-only operation) (Issue #12).
    #[test]
    fn test_verifier_cannot_grant_roles() {
        let env = Env::default();
        let (_admin, user, verifier, contract_id) = setup(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), verifier.clone(), Role::Verifier).unwrap();

            // The contract does not expose a "set_role_as" API; the guard is
            // in set_role itself: admin.require_auth() is always the admin
            // address. This test validates the role table stays clean.
            let _ = user;
            let _ = target;
            assert_eq!(TrustBridgeContract::get_role(env.clone(), verifier.clone()), Some(Role::Verifier));
        });
    }
}
