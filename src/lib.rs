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
pub use storage::{ContributorRecord, Role, Stats};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::storage::{
    add_to_index, get_admin, get_cooldown as storage_get_cooldown, get_count, get_index,
    get_last_upgrade, get_record, get_role as storage_get_role, get_stats as read_stats,
    get_verified_count as storage_get_verified_count, get_version as storage_get_version,
    has_record, is_paused as storage_is_paused, remove_from_index, remove_record,
    remove_role as storage_remove_role, require_initialized, require_not_paused,
    set_cooldown as storage_set_cooldown, set_count, set_last_upgrade,
    set_paused as storage_set_paused, set_record, set_role as storage_set_role, set_verified_count,
    set_version, ADMIN_KEY,
};
use crate::storage::{is_admin_caller, is_in_cooldown, is_paused, set_last_action, set_paused};
use crate::batch::BatchConfig;
use crate::storage::extend_record_ttl;

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
        storage_get_version(&env)
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

        let version = storage_get_version(&env);
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

        let current = storage_get_version(&env);
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
        get_version(&env).unwrap_or_else(|| CONTRACT_VERSION.to_tuple())
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

    /// Extends the storage TTL of registry records so they are not archived.
    ///
    /// Soroban persistent entries expire unless their TTL is extended. Reads and
    /// writes extend as a side effect, but a record nobody touches for ~30 days
    /// is archived and becomes unreadable until restored — so a registry with a
    /// long tail of inactive contributors silently loses its cold entries.
    ///
    /// This is the keeper operation that prevents that: an off-chain job walks
    /// the index and calls this periodically for entries approaching expiry.
    ///
    /// Permissionless by design. Extending a TTL only ever preserves data —
    /// there is no state an attacker could corrupt by calling it, and gating it
    /// behind admin auth would mean the registry decays whenever the admin key
    /// is unavailable. The caller pays the fee, which is its own rate limit.
    ///
    /// Returns the number of entries actually extended. Usernames that are not
    /// registered are skipped rather than erroring: the keeper's list is built
    /// off-chain and can lag behind removals.
    pub fn extend_registry_ttl(
        env: Env,
        usernames: Vec<String>,
    ) -> Result<u32, ContractError> {
        require_initialized(&env)?;

        // Bounded for the same reason as batch_verify: an unbounded Vec could
        // exhaust the ledger's CPU budget and fail after partial work, leaving
        // the keeper unable to tell which entries were extended.
        let config = BatchConfig::default();
        if !config.is_valid_batch_size(usernames.len()) {
            return Err(ContractError::InvalidBatchSize);
        }

        let mut extended: u32 = 0;
        for username in usernames.iter() {
            if extend_record_ttl(&env, &username) {
                extended = extended.saturating_add(1);
            }
        }

        Ok(extended)
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

    /// Toggles contract pause state. Admin-only (Issue #3).
    pub fn set_paused(env: Env, paused: bool) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        set_paused_state(&env, paused);
        Ok(())
    }

    /// Checks if the contract is paused (Issue #3).
    pub fn is_paused(env: Env) -> bool {
        storage_is_paused(&env)
    }

    /// Marks a contributor as verified after an off-chain GitHub identity check. Admin-only.
    pub fn verify(env: Env, github_username: String) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

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
        require_not_paused(&env)?;

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

    // --- Reference event indexer hardening: admin/pause/roles/cooldown (Wave #33) ---

    /// Pauses the contract. Admin-only.
    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        require_initialized(&env)?;
        caller.require_auth();
        if !is_admin_caller(&env, &caller) {
            return Err(ContractError::NotAuthorized);
        }
        set_paused(&env, true);
        Ok(())
    }

    /// Unpauses the contract. Admin-only.
    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        require_initialized(&env)?;
        caller.require_auth();
        if !is_admin_caller(&env, &caller) {
            return Err(ContractError::NotAuthorized);
        }
        set_paused(&env, false);
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_contract_paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Returns true if `caller` holds the admin role.
    pub fn has_admin_role(env: Env, caller: Address) -> bool {
        is_admin_caller(&env, &caller)
    }

    /// Records that `github_username` performed a registry-mutating action now,
    /// for cooldown enforcement by callers.
    pub fn record_action(env: Env, github_username: String) {
        set_last_action(&env, &github_username, env.ledger().timestamp());
    }

    /// Returns true if `github_username` is still within the cooldown window.
    pub fn is_registration_in_cooldown(env: Env, github_username: String) -> bool {
        is_in_cooldown(&env, &github_username)
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
            let result =
                TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone());
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

            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).is_none()
            );
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
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none()
            );
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
            let result =
                TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "missing"));
            assert_eq!(result, Err(ContractError::NotRegistered));
        });
    }

    #[test]
    fn test_double_verify_fails() {
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
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::revoke_verification(env.clone(), username(&env, "octocat"))
                .unwrap();
            assert_eq!(TrustBridgeContract::get_verified_count(env.clone()), 0);
            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }

    #[test]
    fn test_revoke_verification_nonverified_fails() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let result =
                TrustBridgeContract::revoke_verification(env.clone(), username(&env, "octocat"));
            assert_eq!(result, Err(ContractError::NotVerified));
        });
    }

    #[test]
    fn test_register_two_users_keeps_addresses() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "alice"))
                    .unwrap()
                    .stellar_address,
                user1
            );
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "bob"))
                    .unwrap()
                    .stellar_address,
                user2
            );
        });
    }

    #[test]
    fn test_owner_can_remove_registration() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat"))
                .unwrap();
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).is_none()
            );
        });
    }

    #[test]
    fn test_readding_removed_user_increments_count() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user.clone(), username(&env, "octocat"))
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }

    #[test]
    fn test_export_skips_removed_records() {
        let env = Env::default();
        let (_admin, user1, user2, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice"))
                .unwrap();
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
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
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
        assert_eq!(ContractError::NotVerified.code(), 6);
        assert_eq!(ContractError::InvalidUsername.code(), 7);
    }

    #[test]
    fn test_updated_registration_preserves_count() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone())
                .unwrap();
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 1);
        });
    }

    #[test]
    fn test_unverified_update_stays_unverified() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone())
                .unwrap();
            let record =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert!(!record.verified);
        });
    }

    #[test]
    fn test_verified_same_address_reregister_keeps_count() {
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
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone())
                .unwrap();
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
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice"))
                .unwrap();
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
            TrustBridgeContract::register(env.clone(), username(&env, "alice"), user1.clone())
                .unwrap();
            TrustBridgeContract::register(env.clone(), username(&env, "bob"), user2.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove(env.clone(), user1.clone(), username(&env, "alice"))
                .unwrap();
            assert_eq!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "bob"))
                    .unwrap()
                    .stellar_address,
                user2
            );
        });
    }

    #[test]
    fn test_verify_after_reregister_new_address() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), other.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat"))
                    .unwrap()
                    .verified
            );
        });
    }

    #[test]
    fn test_repeated_missing_lookups_are_stable() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none()
            );
            assert!(
                TrustBridgeContract::get_address(env.clone(), username(&env, "missing")).is_none()
            );
        });
    }

    #[test]
    fn test_verify_after_address_update_targets_new_address_wave_49() {
        let env = Env::default();
        let (_admin, old_user, new_user, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), old_user.clone())
                .unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), new_user.clone())
                .unwrap();
        });

        env.as_contract(&contract_id, || {
            let after_update =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(after_update.stellar_address, new_user);
            assert!(!after_update.verified);
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 0);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::verify(env.clone(), username(&env, "octocat")).unwrap();
        });

        env.as_contract(&contract_id, || {
            let after_verify =
                TrustBridgeContract::get_address(env.clone(), username(&env, "octocat")).unwrap();
            assert_eq!(after_verify.stellar_address, new_user);
            assert!(after_verify.verified);
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).verified, 1);
        });
    }

    #[test]
    fn test_cold_start_register_exposes_dashboard_state_wave_50() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::get_all_registered(env.clone())
                    .unwrap()
                    .len(),
                0
            );
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::register(env.clone(), username(&env, "octocat"), user.clone())
                .unwrap();
            assert_eq!(
                TrustBridgeContract::get_all_registered(env.clone())
                    .unwrap()
                    .len(),
                1
            );
        });
    }

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
            let reg_res =
                TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone());
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
            let reg_res_2 =
                TrustBridgeContract::register(env.clone(), username(&env, "alice"), user.clone());
            assert!(reg_res_2.is_ok());
        });

        let budget = env.cost_estimate().budget();
        (budget.cpu_instruction_cost(), budget.memory_bytes_cost())
    }

    // ─── Benchmark scaffolding (Wave #7) ─────────────────────────────────────
    //
    // BENCH_SIZES, bench_username and measure_export were referenced by
    // test_bench_export_cpu_cost but never defined, so `make bench-export` could
    // not have run. Defined here because the TTL extender benchmark needs the
    // same scaffolding, and a benchmark issue is the right place to make the
    // existing benchmark work.

    /// Registry sizes each benchmark sweeps, smallest first.
    ///
    /// The spread has to be wide enough that super-linear growth is
    /// distinguishable from per-entry overhead, but small enough to stay inside
    /// the test budget.
    const BENCH_SIZES: [u32; 4] = [10, 25, 50, 100];

    /// Deterministic distinct username for benchmark entry `i`.
    ///
    /// Fixed-width so every key serialises to the same length and entry size
    /// does not drift across the sweep — otherwise the larger sizes would carry
    /// slightly wider keys and blur the cost curve.
    fn bench_username(env: &Env, i: u32) -> String {
        let mut buf = [b'0'; 8];
        let mut n = i;
        let mut idx = buf.len();
        while idx > 0 {
            idx -= 1;
            buf[idx] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        String::from_str(env, core::str::from_utf8(&buf).unwrap())
    }

    /// Registers `size` contributors, then measures one full registry export.
    fn measure_export(size: u32) -> (u64, u64) {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            for i in 0..size {
                let _ = TrustBridgeContract::register(
                    env.clone(),
                    bench_username(&env, i),
                    user.clone(),
                );
            }
        });

        // Reset so the measurement covers the export alone, not the setup.
        env.cost_estimate().budget().reset_default();

        env.as_contract(&contract_id, || {
            let _ = TrustBridgeContract::get_all_registered(env.clone());
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

    /// Registers `size` contributors, then measures one `extend_registry_ttl`
    /// call covering all of them.
    ///
    /// Only the extension is metered: registration happens before the budget is
    /// read, so setup cost does not pollute the number.
    fn measure_ttl_extension(size: u32) -> (u64, u64, u32) {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);

        env.mock_all_auths();

        let mut names = Vec::new(&env);
        env.as_contract(&contract_id, || {
            for i in 0..size {
                let name = bench_username(&env, i);
                let _ = TrustBridgeContract::register(
                    env.clone(),
                    name.clone(),
                    user.clone(),
                );
                names.push_back(name);
            }
        });

        // Reset so the measurement below covers the extension alone.
        env.cost_estimate().budget().reset_default();

        let extended = env.as_contract(&contract_id, || {
            TrustBridgeContract::extend_registry_ttl(env.clone(), names.clone())
                .expect("extend_registry_ttl should succeed for a valid batch")
        });

        let budget = env.cost_estimate().budget();
        (
            budget.cpu_instruction_cost(),
            budget.memory_bytes_cost(),
            extended,
        )
    }

    /// Benchmark for the TTL extender keeper operation (Wave #7).
    ///
    /// Run via `make bench-ttl`. The assertions below are the part that makes
    /// this a regression gate rather than a report: a keeper's per-entry cost
    /// determines how many entries it can refresh per transaction, so
    /// super-linear growth is a real operational failure, not just a slow test.
    #[test]
    fn test_bench_ttl_extender_cpu_cost() {
        std::println!("operation,size,cpu_instructions,memory_bytes,cpu_per_entry");

        let mut previous_cpu = 0u64;
        let mut baseline: Option<(u32, u64)> = None;
        let mut largest: Option<(u32, u64)> = None;

        for size in BENCH_SIZES {
            let (cpu, mem, extended) = measure_ttl_extension(size);
            std::println!(
                "extend_registry_ttl,{},{},{},{}",
                size,
                cpu,
                mem,
                cpu / (size as u64).max(1)
            );

            assert_eq!(
                extended, size,
                "extend_registry_ttl reported {extended} extensions for {size} registered entries"
            );
            assert!(cpu > 0, "TTL extension at size {size} was not metered");
            // Every entry is touched, so cost is monotonic in batch size. A drop
            // means the extender stopped visiting some of them.
            assert!(
                cpu >= previous_cpu,
                "TTL extension CPU cost dropped at size {size}: {cpu} < {previous_cpu}"
            );

            previous_cpu = cpu;
            baseline.get_or_insert((size, cpu));
            largest = Some((size, cpu));
        }

        let (small_size, small_cpu) = baseline.unwrap();
        let (large_size, large_cpu) = largest.unwrap();

        // Extension is a flat per-entry operation with no index scan. Same 3x
        // headroom as the export benchmark: normal per-entry overhead passes,
        // quadratic growth fails.
        let ceiling = small_cpu * ((large_size / small_size) as u64) * 3;
        assert!(
            large_cpu <= ceiling,
            "TTL extension CPU cost grew super-linearly: {large_cpu} at size {large_size} exceeds ceiling {ceiling}"
        );
    }

    #[test]
    fn test_extend_registry_ttl_skips_unregistered() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            let _ = TrustBridgeContract::register(
                env.clone(),
                username(&env, "alice"),
                user.clone(),
            );

            let mut names = Vec::new(&env);
            names.push_back(username(&env, "alice"));
            // Never registered — the keeper's off-chain list can lag removals.
            names.push_back(username(&env, "ghost"));

            let extended = TrustBridgeContract::extend_registry_ttl(env.clone(), names)
                .expect("a batch with unknown usernames should still succeed");
            assert_eq!(extended, 1, "only the registered entry should be extended");
        });
    }

    #[test]
    fn test_extend_registry_ttl_rejects_bad_batch_size() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            let empty = Vec::new(&env);
            assert_eq!(
                TrustBridgeContract::extend_registry_ttl(env.clone(), empty),
                Err(ContractError::InvalidBatchSize),
                "an empty batch should be rejected rather than silently succeeding"
            );
        });
    }

    #[test]
    fn test_roles_management() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::get_role(env.clone(), user.clone()),
                None
            );
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), user.clone(), Role::Upgrader).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::get_role(env.clone(), user.clone()),
                Some(Role::Upgrader)
            );
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_role(env.clone(), other.clone(), Role::Verifier).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::get_role(env.clone(), other.clone()),
                Some(Role::Verifier)
            );
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::remove_role(env.clone(), user.clone()).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::get_role(env.clone(), user.clone()),
                None
            );
        });
    }

    #[test]
    fn test_cooldown_and_upgrade() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_cooldown(env.clone()), 0);
        });

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::set_cooldown(env.clone(), 3600).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_cooldown(env.clone()), 3600);
        });

        let wasm_bytes = soroban_sdk::Bytes::from_slice(
            &env,
            include_bytes!("../target/wasm32v1-none/release/trustbridge_contract.wasm"),
        );
        let wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes.clone());

        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let res = TrustBridgeContract::upgrade(env.clone(), wasm_hash.clone());
            assert!(res.is_ok());
        });

        // Immediate upgrade should fail due to active cooldown
        let wasm_hash_2 = env.deployer().upload_contract_wasm(wasm_bytes);
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let res_2 = TrustBridgeContract::upgrade(env.clone(), wasm_hash_2);
            assert_eq!(res_2, Err(ContractError::CooldownActive));
        });
    }

    #[test]
    fn test_migration_version_increment() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 0, 0));
        });

        // Migration to equal/lower version fails
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            let err_res = TrustBridgeContract::migrate(env.clone(), (1, 0, 0));
            assert_eq!(err_res, Err(ContractError::InvalidVersion));
        });

        // Migration to higher version succeeds
        env.mock_all_auths();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::migrate(env.clone(), (1, 1, 0)).unwrap();
        });

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::get_version(env.clone()), (1, 1, 0));
        });
    }

    #[test]
    fn test_new_error_codes() {
        assert_eq!(ContractError::Paused.code(), 7);
        assert_eq!(ContractError::CooldownActive.code(), 8);
        assert_eq!(ContractError::InvalidVersion.code(), 9);
        assert_eq!(ContractError::InvalidRole.code(), 10);
    }
}
