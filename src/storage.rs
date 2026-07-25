use soroban_sdk::{symbol_short, Address, Env, String, Symbol, Vec};

use crate::ContractError;

pub const REG_KEY: Symbol = symbol_short!("reg");
pub const ADMIN_KEY: Symbol = symbol_short!("admin");
pub const COUNT_KEY: Symbol = symbol_short!("count");
pub const VCOUNT_KEY: Symbol = symbol_short!("vcount");
pub const INDEX_KEY: Symbol = symbol_short!("idx");

#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
pub struct ContributorRecord {
    pub stellar_address: Address,
    pub registered_at: u64,
    pub verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
pub struct Stats {
    pub total: u32,
    pub verified: u32,
}

pub fn require_initialized(env: &Env) -> Result<(), ContractError> {
    if env.storage().instance().has(&ADMIN_KEY) {
        Ok(())
    } else {
        Err(ContractError::NotInitialized)
    }
}

pub fn get_admin(env: &Env) -> Result<Address, ContractError> {
    require_initialized(env)?;
    env.storage()
        .instance()
        .get(&ADMIN_KEY)
        .ok_or(ContractError::NotInitialized)
}

pub fn get_record(env: &Env, github_username: &String) -> Option<ContributorRecord> {
    env.storage()
        .persistent()
        .get(&(REG_KEY, github_username.clone()))
}

pub fn set_record(env: &Env, github_username: &String, record: &ContributorRecord) {
    env.storage()
        .persistent()
        .set(&(REG_KEY, github_username.clone()), record);
}

pub fn remove_record(env: &Env, github_username: &String) {
    env.storage()
        .persistent()
        .remove(&(REG_KEY, github_username.clone()));
}

pub fn get_count(env: &Env) -> u32 {
    env.storage().instance().get(&COUNT_KEY).unwrap_or(0)
}

pub fn set_count(env: &Env, count: u32) {
    env.storage().instance().set(&COUNT_KEY, &count);
}

pub fn get_verified_count(env: &Env) -> u32 {
    env.storage().instance().get(&VCOUNT_KEY).unwrap_or(0)
}

pub fn set_verified_count(env: &Env, count: u32) {
    env.storage().instance().set(&VCOUNT_KEY, &count);
}

pub fn get_index(env: &Env) -> Vec<String> {
    env.storage()
        .instance()
        .get(&INDEX_KEY)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_index(env: &Env, index: &Vec<String>) {
    env.storage().instance().set(&INDEX_KEY, index);
}

pub fn add_to_index(env: &Env, github_username: &String) {
    let mut index = get_index(env);
    index.push_back(github_username.clone());
    set_index(env, &index);
}

pub fn remove_from_index(env: &Env, github_username: &String) {
    let index = get_index(env);
    let mut next = Vec::new(env);
    for i in 0..index.len() {
        let username = index.get(i).unwrap();
        if username != *github_username {
            next.push_back(username);
        }
    }
    set_index(env, &next);
}

pub fn build_stats(total: u32, verified: u32) -> Stats {
    Stats { total, verified }
}

pub fn get_stats(env: &Env) -> Stats {
    build_stats(get_count(env), get_verified_count(env))
}

pub fn has_record(env: &Env, github_username: &String) -> bool {
    get_record(env, github_username).is_some()
}

// --- Orphan module integration: index TTL + pagination (Wave #31) ---

/// Ledger threshold (~1 day at 5s/ledger) below which the instance TTL is bumped.
pub const INDEX_TTL_THRESHOLD: u32 = 17_280;
/// Ledger count (~7 days at 5s/ledger) the instance TTL is extended to.
pub const INDEX_TTL_EXTEND_TO: u32 = 120_960;

/// Extends the instance storage TTL (which backs the registry index) so the
/// index is not evicted while a Wave is actively registering contributors.
pub fn bump_index_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INDEX_TTL_THRESHOLD, INDEX_TTL_EXTEND_TO);
}

/// Returns a page of `limit` usernames from the registry index starting at
/// `offset`. Out-of-range offsets or a zero limit yield an empty page.
pub fn get_index_page(env: &Env, offset: u32, limit: u32) -> Vec<String> {
    let index = get_index(env);
    let len = index.len();
    let mut page = Vec::new(env);

    if limit == 0 || offset >= len {
        return page;
    }

    let end = core::cmp::min(offset.saturating_add(limit), len);
    let mut i = offset;
    while i < end {
        page.push_back(index.get(i).unwrap());
        i += 1;
    }
    page
}

// --- Reference event indexer hardening: pause + role + cooldown (Wave #33) ---

pub const PAUSED_KEY: Symbol = symbol_short!("paused");
pub const LASTACT_KEY: Symbol = symbol_short!("lastact");
/// Minimum seconds a github_username must wait between registry-mutating actions.
pub const COOLDOWN_SECONDS: u64 = 60;

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&PAUSED_KEY).unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&PAUSED_KEY, &paused);
}

pub fn get_last_action(env: &Env, github_username: &String) -> Option<u64> {
    env.storage()
        .persistent()
        .get(&(LASTACT_KEY, github_username.clone()))
}

pub fn set_last_action(env: &Env, github_username: &String, timestamp: u64) {
    env.storage()
        .persistent()
        .set(&(LASTACT_KEY, github_username.clone()), &timestamp);
}

/// Returns true if `github_username` acted within the cooldown window.
pub fn is_in_cooldown(env: &Env, github_username: &String) -> bool {
    match get_last_action(env, github_username) {
        Some(last) => env.ledger().timestamp() < last.saturating_add(COOLDOWN_SECONDS),
        None => false,
    }
}

/// Returns true if `caller` is the configured contract admin (role check).
pub fn is_admin_caller(env: &Env, caller: &Address) -> bool {
    match get_admin(env) {
        Ok(admin) => admin == *caller,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrustBridgeContract;

    fn username(env: &Env, name: &str) -> String {
        String::from_str(env, name)
    }

    #[test]
    fn test_get_index_page_returns_requested_slice() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            add_to_index(&env, &username(&env, "alice"));
            add_to_index(&env, &username(&env, "bob"));
            add_to_index(&env, &username(&env, "carol"));

            let page = get_index_page(&env, 1, 2);
            assert_eq!(page.len(), 2);
            assert_eq!(page.get(0).unwrap(), username(&env, "bob"));
            assert_eq!(page.get(1).unwrap(), username(&env, "carol"));
        });
    }

    #[test]
    fn test_get_index_page_out_of_bounds_is_empty() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            add_to_index(&env, &username(&env, "alice"));

            assert_eq!(get_index_page(&env, 5, 2).len(), 0);
            assert_eq!(get_index_page(&env, 0, 0).len(), 0);
        });
    }

    #[test]
    fn test_bump_index_ttl_does_not_panic() {
        let env = Env::default();
        let contract_id = env.register(TrustBridgeContract, ());
        env.as_contract(&contract_id, || {
            add_to_index(&env, &username(&env, "alice"));
            bump_index_ttl(&env);
        });
    }
}
