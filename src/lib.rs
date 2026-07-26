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
pub use version::Version;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::storage::{
    add_to_index, get_admin, get_cooldown as storage_get_cooldown, get_count, get_index,
    get_last_upgrade, get_record, get_registered_paginated_internal, get_role as storage_get_role,
    get_stats as read_stats, get_verified_count as storage_get_verified_count,
    get_version as storage_get_version, has_record, is_admin_caller, is_in_cooldown,
    is_paused as storage_is_paused, remove_from_index, remove_record,
    remove_role as storage_remove_role, require_initialized, require_not_paused,
    set_cooldown as storage_set_cooldown, set_count, set_last_action, set_last_upgrade,
    set_paused as set_paused_state, set_record, set_role as storage_set_role, set_verified_count,
    set_version, ADMIN_KEY,
};
use crate::utils::{eq_ignore_ascii_case, is_valid_github_username, MAX_USERNAME_LEN};

/// Version this WASM was built at. Instances whose stored version predates
/// version tracking fall back to this.
pub const CONTRACT_VERSION: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
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
        set_paused_state(&env, false);
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

        set_paused_state(&env, true);
        let timestamp = env.ledger().timestamp();
        PausedEvent { admin, timestamp }.publish(&env);
        Ok(())
    }

    /// Unpauses contract state mutations. Admin-only.
    pub fn unpause(env: Env) -> Result<(), ContractError> {
        require_initialized(&env)?;
        let admin = get_admin(&env)?;
        admin.require_auth();

        set_paused_state(&env, false);
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
        if require_initialized(&env).is_err() {
            return CONTRACT_VERSION.to_tuple();
        }
        storage_get_version(&env)
    }

    /// Reports whether the deployed contract satisfies a client's minimum
    /// required version. Bindings consumers call this before invoking, so a
    /// stale client fails fast instead of on an unexpected ABI.
    pub fn is_compatible(env: Env, major: u32, minor: u32, patch: u32) -> bool {
        Version::from_tuple(Self::version(env))
            .is_compatible_with(Version::new(major, minor, patch))
    }

    /// Returns the maximum accepted GitHub username length.
    ///
    /// Clients read this instead of hardcoding 39, so a future relaxation of
    /// the guard does not require a client release.
    pub fn max_username_len(_env: Env) -> u32 {
        MAX_USERNAME_LEN
    }

    /// Reports whether `github_username` would pass the `register` guard.
    /// Lets a dashboard validate input before asking the user to sign.
    pub fn is_username_valid(_env: Env, github_username: String) -> bool {
        is_valid_github_username(&github_username)
    }

    /// Case-insensitive username equality, matching GitHub's own semantics.
    ///
    /// Off-chain verification workflows use this to match a registration
    /// against a GitHub identity without depending on the stored casing.
    pub fn usernames_match(_env: Env, a: String, b: String) -> bool {
        eq_ignore_ascii_case(&a, &b)
    }

    /// Registers or updates a GitHub username → Stellar address mapping.
    ///
    /// The caller must authenticate as `stellar_address`. The username must be
    /// 1 to `MAX_USERNAME_LEN` (39) characters of alphanumerics, hyphens, and
    /// underscores, starting and ending alphanumeric, or the call fails with
    /// `InvalidUsername`.
    ///
    /// Re-pointing an existing registration at a different address also
    /// requires authentication from the address currently registered, so a
    /// username cannot be taken over by whoever calls `register` next.
    pub fn register(
        env: Env,
        github_username: String,
        stellar_address: Address,
    ) -> Result<(), ContractError> {
        require_initialized(&env)?;
        require_not_paused(&env)?;

        // Validate before auth: a malformed username is rejected at the
        // cheapest point, before any signature check or storage read.
        if !is_valid_github_username(&github_username) {
            return Err(ContractError::InvalidUsername);
        }

        stellar_address.require_auth();

        let timestamp = env.ledger().timestamp();
        let existing = get_record(&env, &github_username);

        // Self-auth enforcement: transferring a username away from its current
        // owner needs that owner's signature too.
        if let Some(ref old) = existing {
            if old.stellar_address != stellar_address {
                old.stellar_address.require_auth();
            }
        }

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

        let page = crate::storage::get_index_page(&env, offset, limit);
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

    /// Returns whether the contract is currently paused.
    ///
    /// Alias of `is_paused` kept for the reference indexer, which reads this
    /// name.
    pub fn is_contract_paused(env: Env) -> bool {
        storage_is_paused(&env)
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
extern crate alloc;
#[cfg(test)]
extern crate std;

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _, Ledger as _},
        Address, Env, Event as _, String, TryFromVal,
    };

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
        assert_eq!(ContractError::Paused.code(), 7);
        assert_eq!(ContractError::CooldownActive.code(), 8);
        assert_eq!(ContractError::InvalidVersion.code(), 9);
        assert_eq!(ContractError::InvalidRole.code(), 10);
        assert_eq!(ContractError::InvalidUsername.code(), 11);
    }

    #[test]
    fn test_error_from_code_is_inverse_of_code() {
        for variant in [
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
            ContractError::InvalidUsername,
        ] {
            assert_eq!(ContractError::from_code(variant.code()), Some(variant));
        }
        assert_eq!(ContractError::from_code(0), None);
        assert_eq!(ContractError::from_code(12), None);
    }

    // --- Issue #69: max username length guard ---

    #[test]
    fn test_register_rejects_over_length_username() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();

        // 40 characters: one past MAX_USERNAME_LEN.
        let too_long = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(too_long.len(), MAX_USERNAME_LEN + 1);

        env.as_contract(&contract_id, || {
            assert_eq!(
                TrustBridgeContract::register(env.clone(), too_long.clone(), user.clone()),
                Err(ContractError::InvalidUsername)
            );
            // The rejected username must leave no trace in the registry.
            assert!(!TrustBridgeContract::has_record(env.clone(), too_long));
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
        });
    }

    #[test]
    fn test_register_accepts_username_at_max_length() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();

        // Exactly 39 characters.
        let at_max = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(at_max.len(), MAX_USERNAME_LEN);

        env.as_contract(&contract_id, || {
            assert!(
                TrustBridgeContract::register(env.clone(), at_max.clone(), user.clone()).is_ok()
            );
            assert!(TrustBridgeContract::has_record(env.clone(), at_max));
        });
    }

    #[test]
    fn test_register_rejects_empty_and_malformed_usernames() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();

        env.as_contract(&contract_id, || {
            for bad in ["", "-lead", "trail-", "has space", "at@sign"] {
                assert_eq!(
                    TrustBridgeContract::register(env.clone(), username(&env, bad), user.clone()),
                    Err(ContractError::InvalidUsername),
                    "expected {bad:?} to be rejected"
                );
            }
            assert_eq!(TrustBridgeContract::get_stats(env.clone()).total, 0);
        });
    }

    #[test]
    fn test_max_username_len_is_exposed() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert_eq!(TrustBridgeContract::max_username_len(env.clone()), 39);
            assert!(TrustBridgeContract::is_username_valid(
                env.clone(),
                username(&env, "octocat")
            ));
            assert!(!TrustBridgeContract::is_username_valid(
                env.clone(),
                username(&env, "octo cat")
            ));
        });
    }

    // --- Issue #68: username case normalization ---

    #[test]
    fn test_usernames_match_is_case_insensitive() {
        let env = Env::default();
        let (_admin, _user, _other, contract_id) = setup(&env);

        env.as_contract(&contract_id, || {
            assert!(TrustBridgeContract::usernames_match(
                env.clone(),
                username(&env, "OctoCat"),
                username(&env, "octocat")
            ));
            assert!(!TrustBridgeContract::usernames_match(
                env.clone(),
                username(&env, "octocat"),
                username(&env, "octocat1")
            ));
        });
    }

    // --- Issue #72: register self-auth enforcement ---

    #[test]
    fn test_register_transfer_requires_current_owner_auth() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        let client = TrustBridgeContractClient::new(&env, &contract_id);
        let name = username(&env, "octocat");

        env.mock_all_auths();
        client.register(&name, &user);

        // Re-point the registration at `other`, authorizing only `other`.
        // The current owner's signature is missing, so the call must fail.
        env.set_auths(&[]);
        let res = client.try_register(&name, &other);
        assert!(
            res.is_err(),
            "takeover succeeded without the current owner's authorization"
        );

        // The registration is unchanged.
        env.mock_all_auths();
        assert_eq!(client.get_address(&name).unwrap().stellar_address, user);
    }

    #[test]
    fn test_register_transfer_succeeds_with_both_auths() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        let client = TrustBridgeContractClient::new(&env, &contract_id);
        let name = username(&env, "octocat");

        env.mock_all_auths();
        client.register(&name, &user);
        client.register(&name, &other);

        assert_eq!(client.get_address(&name).unwrap().stellar_address, other);
        assert_eq!(client.get_stats().total, 1);
    }

    // --- Issue #64: RemovedEvent payload ---

    #[test]
    fn test_removed_event_payload_is_complete() {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        let client = TrustBridgeContractClient::new(&env, &contract_id);
        let name = username(&env, "octocat");

        env.mock_all_auths();
        client.register(&name, &user);

        env.ledger().set_timestamp(1_700_000_000);
        client.remove(&user, &name);

        // `remove` must publish exactly one event, and that event must be a
        // fully-populated RemovedEvent: the username as topic, and the removed
        // address plus the removal timestamp as data. An indexer replaying only
        // this event has to be able to reconstruct the record it is retiring,
        // so every field is asserted rather than just the event's presence.
        let expected = RemovedEvent {
            github_username: name.clone(),
            stellar_address: user.clone(),
            timestamp: 1_700_000_000,
        };

        assert_eq!(
            env.events().all(),
            soroban_sdk::vec![
                &env,
                (
                    contract_id.clone(),
                    expected.topics(&env),
                    expected.data(&env),
                )
            ],
            "RemovedEvent payload or topics changed"
        );

        // Pin the topic shape independently of the struct, so renaming the
        // event or dropping the username topic breaks this test rather than
        // silently breaking every downstream subscriber's filter.
        let topics = expected.topics(&env);
        assert_eq!(topics.len(), 2, "RemovedEvent must have 2 topics");
        assert_eq!(
            soroban_sdk::Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(),
            soroban_sdk::Symbol::new(&env, "removed_event"),
            "RemovedEvent topic symbol changed"
        );
        assert_eq!(
            String::try_from_val(&env, &topics.get(1).unwrap()).unwrap(),
            name,
            "RemovedEvent username topic changed"
        );
    }

    #[test]
    fn test_removed_event_not_published_on_failed_remove() {
        let env = Env::default();
        let (_admin, user, other, contract_id) = setup(&env);
        let client = TrustBridgeContractClient::new(&env, &contract_id);
        let name = username(&env, "octocat");

        env.mock_all_auths();
        client.register(&name, &user);

        // A caller who is neither the registrant nor the admin is rejected,
        // and must not leave a RemovedEvent behind for indexers to act on.
        assert!(client.try_remove(&other, &name).is_err());
        assert_eq!(
            env.events().all(),
            soroban_sdk::vec![&env],
            "failed remove published an event"
        );
        assert!(client.has_record(&name));
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
    }

    /// Comparison counts the case-normalization benchmark sweeps. Normalization
    /// touches no ledger entries, so it is not footprint-bound.
    const BENCH_SIZES: [u32; 4] = [10, 50, 100, 200];

    /// Registry sizes the full-export benchmark sweeps.
    ///
    /// Capped below 100: `get_all_registered` reads one ledger entry per
    /// record, and Soroban rejects an invocation whose footprint exceeds 100
    /// entries. That ceiling is the reason `get_registered_page` exists — see
    /// `test_bench_export_footprint_ceiling`.
    const EXPORT_BENCH_SIZES: [u32; 4] = [10, 20, 40, 80];

    /// Registers `size` contributors and measures the metered cost of a single
    /// full export. Returns `(cpu_instructions, memory_bytes)`.
    fn measure_export(size: u32) -> (u64, u64) {
        let env = Env::default();
        let (_admin, user, _other, contract_id) = setup(&env);
        env.mock_all_auths();

        // Each registration runs in its own frame: `require_auth` for the same
        // address twice in one frame is an Auth(ExistingValue) error.
        for i in 0..size {
            let mut name = alloc::string::String::from("bench");
            name.push_str(&alloc::format!("{i}"));
            let name = String::from_str(&env, &name);
            env.as_contract(&contract_id, || {
                TrustBridgeContract::register(env.clone(), name.clone(), user.clone()).unwrap();
            });
        }

        env.cost_estimate().budget().reset_default();
        env.as_contract(&contract_id, || {
            TrustBridgeContract::get_all_registered(env.clone()).unwrap();
        });

        let budget = env.cost_estimate().budget();
        (budget.cpu_instruction_cost(), budget.memory_bytes_cost())
    }

    /// Measures the metered cost of `size` case-insensitive username
    /// comparisons — the normalization step an off-chain verifier performs per
    /// candidate match. Returns `(cpu_instructions, memory_bytes)`.
    fn measure_case_normalization(size: u32) -> (u64, u64) {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());

        // Mixed-case on one side, lower-case on the other, so every comparison
        // exercises the folding path rather than an early length mismatch.
        let upper = String::from_str(&env, "OctoCat-Dev_01");
        let lower = String::from_str(&env, "octocat-dev_01");

        env.cost_estimate().budget().reset_default();
        env.as_contract(&contract_id, || {
            for _ in 0..size {
                assert!(TrustBridgeContract::usernames_match(
                    env.clone(),
                    upper.clone(),
                    lower.clone()
                ));
            }
        });

        let budget = env.cost_estimate().budget();
        (budget.cpu_instruction_cost(), budget.memory_bytes_cost())
    }

    /// Benchmark for issue #68: username case normalization must stay linear
    /// in the number of comparisons and must not allocate per comparison.
    #[test]
    fn test_bench_username_case_normalization() {
        std::println!("operation,size,cpu_instructions,memory_bytes");

        let mut previous_cpu = 0u64;
        let mut baseline: Option<(u32, u64)> = None;
        let mut largest: Option<(u32, u64)> = None;

        for size in BENCH_SIZES {
            let (cpu, mem) = measure_case_normalization(size);
            std::println!("usernames_match,{},{},{}", size, cpu, mem);

            assert!(cpu > 0, "normalization at size {size} was not metered");
            assert!(
                cpu >= previous_cpu,
                "normalization CPU cost dropped at size {size}: {cpu} < {previous_cpu}"
            );

            previous_cpu = cpu;
            baseline.get_or_insert((size, cpu));
            largest = Some((size, cpu));
        }

        let (small_size, small_cpu) = baseline.unwrap();
        let (large_size, large_cpu) = largest.unwrap();

        // Comparison is a fixed-width stack scan, so cost is linear in the
        // number of calls. 3x headroom over the size ratio absorbs per-call
        // overhead while still failing on super-linear growth.
        let ceiling = small_cpu * ((large_size / small_size) as u64) * 3;
        assert!(
            large_cpu <= ceiling,
            "normalization CPU cost grew super-linearly: {large_cpu} at size {large_size} exceeds ceiling {ceiling}"
        );
    }

    #[test]
    fn test_bench_export_cpu_cost() {
        std::println!("operation,size,cpu_instructions,memory_bytes");

        let mut previous_cpu = 0u64;
        let mut baseline: Option<(u32, u64)> = None;
        let mut largest: Option<(u32, u64)> = None;

        for size in EXPORT_BENCH_SIZES {
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
