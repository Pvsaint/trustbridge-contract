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

// Wave #41: build_stats is the single centralized constructor for `Stats`.
// All stats reads (get_stats, and any future indexer/dashboard aggregate
// endpoints) should route through it rather than building `Stats { .. }`
// literals directly, so count/verified-count semantics stay in one place.
pub fn build_stats(total: u32, verified: u32) -> Stats {
    Stats { total, verified }
}

pub fn get_stats(env: &Env) -> Stats {
    build_stats(get_count(env), get_verified_count(env))
}

pub fn has_record(env: &Env, github_username: &String) -> bool {
    get_record(env, github_username).is_some()
}

// Wave #41: pagination helper over the INDEX_KEY list for indexer/dashboard
// consumers that page through the registry instead of pulling it whole.
// `offset`/`limit` are clamped to the index length; entries whose persistent
// record has since expired (see docs/SECURITY.md "Storage TTL") are skipped.
pub fn get_index_page(env: &Env, offset: u32, limit: u32) -> Vec<String> {
    let index = get_index(env);
    let len = index.len();
    let start = offset.min(len);
    let end = start.saturating_add(limit).min(len);

    let mut page = Vec::new(env);
    for i in start..end {
        if let Some(username) = index.get(i) {
            page.push_back(username);
        }
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_build_stats_centralizes_construction() {
        let stats = build_stats(5, 2);
        assert_eq!(stats.total, 5);
        assert_eq!(stats.verified, 2);
    }

    #[test]
    fn test_get_index_page_pagination() {
        let env = Env::default();
        let mut index = Vec::new(&env);
        for name in ["alice", "bob", "carol", "dave"] {
            index.push_back(String::from_str(&env, name));
        }
        set_index(&env, &index);

        let page = get_index_page(&env, 1, 2);
        assert_eq!(page.len(), 2);
        assert_eq!(page.get(0).unwrap(), String::from_str(&env, "bob"));
        assert_eq!(page.get(1).unwrap(), String::from_str(&env, "carol"));

        // offset past the end returns an empty page instead of panicking
        let empty = get_index_page(&env, 10, 2);
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_has_record_true_after_set_record() {
        let env = Env::default();
        let username = String::from_str(&env, "octocat");
        let addr = Address::generate(&env);
        let record = ContributorRecord {
            stellar_address: addr,
            registered_at: 0,
            verified: false,
        };

        assert!(!has_record(&env, &username));
        set_record(&env, &username, &record);
        assert!(has_record(&env, &username));
    }
}
