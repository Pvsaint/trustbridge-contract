use soroban_sdk::{symbol_short, Address, Env, String, Symbol, Vec};

use crate::ContractError;

pub const REG_KEY: Symbol = symbol_short!("reg");
pub const ADMIN_KEY: Symbol = symbol_short!("admin");
pub const COUNT_KEY: Symbol = symbol_short!("count");
pub const VCOUNT_KEY: Symbol = symbol_short!("vcount");
pub const INDEX_KEY: Symbol = symbol_short!("idx");
pub const PAUSED_KEY: Symbol = symbol_short!("pause");
pub const COOLDOWN_KEY: Symbol = symbol_short!("cdown");
pub const LAST_UPG_KEY: Symbol = symbol_short!("lastupg");
pub const VER_KEY: Symbol = symbol_short!("ver");
pub const ROLE_KEY: Symbol = symbol_short!("role");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
#[repr(u32)]
pub enum Role {
    Admin = 1,
    Upgrader = 2,
    Verifier = 3,
}

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

/// Returns the version recorded at initialize time, or `None` for instances
/// deployed before version tracking existed.
pub fn get_version(env: &Env) -> Option<(u32, u32, u32)> {
    env.storage().instance().get(&VERSION_KEY)
}

pub fn set_version(env: &Env, version: &(u32, u32, u32)) {
    env.storage().instance().set(&VERSION_KEY, version);
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

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&PAUSED_KEY).unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&PAUSED_KEY, &paused);
}

pub fn require_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        Err(ContractError::Paused)
    } else {
        Ok(())
    }
}

pub fn get_cooldown(env: &Env) -> u64 {
    env.storage().instance().get(&COOLDOWN_KEY).unwrap_or(0)
}

pub fn set_cooldown(env: &Env, cooldown_seconds: u64) {
    env.storage()
        .instance()
        .set(&COOLDOWN_KEY, &cooldown_seconds);
}

pub fn get_last_upgrade(env: &Env) -> u64 {
    env.storage().instance().get(&LAST_UPG_KEY).unwrap_or(0)
}

pub fn set_last_upgrade(env: &Env, timestamp: u64) {
    env.storage().instance().set(&LAST_UPG_KEY, &timestamp);
}

pub fn get_version(env: &Env) -> (u32, u32, u32) {
    env.storage().instance().get(&VER_KEY).unwrap_or((1, 0, 0))
}

pub fn set_version(env: &Env, version: (u32, u32, u32)) {
    env.storage().instance().set(&VER_KEY, &version);
}

pub fn get_role(env: &Env, address: &Address) -> Option<Role> {
    env.storage().persistent().get(&(ROLE_KEY, address.clone()))
}

pub fn set_role(env: &Env, address: &Address, role: &Role) {
    env.storage()
        .persistent()
        .set(&(ROLE_KEY, address.clone()), role);
}

pub fn remove_role(env: &Env, address: &Address) {
    env.storage()
        .persistent()
        .remove(&(ROLE_KEY, address.clone()));
}

pub fn has_role_or_admin(env: &Env, address: &Address, expected_role: Role) -> bool {
    if let Ok(admin) = get_admin(env) {
        if *address == admin {
            return true;
        }
    }
    match get_role(env, address) {
        Some(Role::Admin) => true,
        Some(r) => r == expected_role,
        None => false,
    }
}
