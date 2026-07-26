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
pub const CHUNK_KEY: Symbol = symbol_short!("chunk");
pub const CHUNK_CNT_KEY: Symbol = symbol_short!("chunkcnt");
pub const LAST_ACT_KEY: Symbol = symbol_short!("lastact");

/// Entries per index chunk. Keeps a single chunk read well under the ledger
/// entry size limit while still amortising reads across pages.
pub const CHUNK_SIZE: u32 = 100;

/// Page size used when a caller passes `limit = 0`.
pub const DEFAULT_PAGE_LIMIT: u32 = 50;
/// Upper bound on a single export page, to keep the response under the
/// transaction result size limit.
pub const MAX_PAGE_LIMIT: u32 = 200;

/// Persistent entries are bumped when their remaining TTL drops below this.
pub const TTL_THRESHOLD: u32 = 100_000;
/// Ledgers of TTL to restore on bump (~60 days at 5s ledgers).
pub const TTL_BUMP: u32 = 1_000_000;

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

#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
pub struct ExportPage {
    pub records: Vec<(String, ContributorRecord)>,
    pub next_cursor: Option<u32>,
    pub total: u32,
    pub has_more: bool,
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
    let key = (REG_KEY, github_username.clone());
    let record: Option<ContributorRecord> = env.storage().persistent().get(&key);
    if record.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
    }
    record
}

pub fn set_record(env: &Env, github_username: &String, record: &ContributorRecord) {
    let key = (REG_KEY, github_username.clone());
    env.storage().persistent().set(&key, record);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

pub fn remove_record(env: &Env, github_username: &String) {
    let key = (REG_KEY, github_username.clone());
    env.storage().persistent().remove(&key);
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

// Chunked Username Index functions (Issue #2)
pub fn get_chunk_count(env: &Env) -> u32 {
    env.storage().instance().get(&CHUNK_CNT_KEY).unwrap_or(0)
}

pub fn set_chunk_count(env: &Env, count: u32) {
    env.storage().instance().set(&CHUNK_CNT_KEY, &count);
}

pub fn get_chunk(env: &Env, chunk_idx: u32) -> Vec<String> {
    let key = (CHUNK_KEY, chunk_idx);
    let chunk: Vec<String> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    if !chunk.is_empty() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
    }
    chunk
}

pub fn set_chunk(env: &Env, chunk_idx: u32, chunk: &Vec<String>) {
    let key = (CHUNK_KEY, chunk_idx);
    env.storage().persistent().set(&key, chunk);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

pub fn add_to_index(env: &Env, github_username: &String) {
    // 1. Maintain legacy single-vec index
    let mut index = get_index(env);
    index.push_back(github_username.clone());
    set_index(env, &index);

    // 2. Maintain chunked index
    let chunk_cnt = get_chunk_count(env);
    if chunk_cnt == 0 {
        let mut first_chunk = Vec::new(env);
        first_chunk.push_back(github_username.clone());
        set_chunk(env, 0, &first_chunk);
        set_chunk_count(env, 1);
    } else {
        let last_idx = chunk_cnt - 1;
        let mut last_chunk = get_chunk(env, last_idx);
        if last_chunk.len() >= CHUNK_SIZE {
            let mut new_chunk = Vec::new(env);
            new_chunk.push_back(github_username.clone());
            set_chunk(env, chunk_cnt, &new_chunk);
            set_chunk_count(env, chunk_cnt + 1);
        } else {
            last_chunk.push_back(github_username.clone());
            set_chunk(env, last_idx, &last_chunk);
        }
    }
}

pub fn remove_from_index(env: &Env, github_username: &String) {
    // 1. Legacy index update
    let index = get_index(env);
    let mut next = Vec::new(env);
    for i in 0..index.len() {
        let username = index.get(i).unwrap();
        if username != *github_username {
            next.push_back(username);
        }
    }
    set_index(env, &next);

    // 2. Chunked index update
    let chunk_cnt = get_chunk_count(env);
    for c in 0..chunk_cnt {
        let chunk = get_chunk(env, c);
        let mut new_chunk = Vec::new(env);
        let mut found = false;
        for i in 0..chunk.len() {
            let username = chunk.get(i).unwrap();
            if username == *github_username {
                found = true;
            } else {
                new_chunk.push_back(username);
            }
        }
        if found {
            set_chunk(env, c, &new_chunk);
            break;
        }
    }
}

/// Returns a slice of the username index: up to `limit` entries starting at
/// `offset`. Out-of-range offsets yield an empty page rather than an error.
pub fn get_index_page(env: &Env, offset: u32, limit: u32) -> Vec<String> {
    let index = get_index(env);
    let mut page = Vec::new(env);

    let effective_limit = if limit == 0 {
        DEFAULT_PAGE_LIMIT
    } else {
        limit.min(MAX_PAGE_LIMIT)
    };

    if offset >= index.len() {
        return page;
    }

    let end = offset.saturating_add(effective_limit).min(index.len());
    for i in offset..end {
        if let Some(username) = index.get(i) {
            page.push_back(username);
        }
    }
    page
}

// Paginated export implementation (Issue #1 & #3)
pub fn get_registered_paginated_internal(
    env: &Env,
    cursor: u32,
    limit: u32,
) -> Result<ExportPage, ContractError> {
    require_initialized(env)?;

    let effective_limit = if limit == 0 {
        DEFAULT_PAGE_LIMIT
    } else if limit > MAX_PAGE_LIMIT {
        MAX_PAGE_LIMIT
    } else {
        limit
    };

    let total_count = get_count(env);
    let mut records = Vec::new(env);

    if cursor >= total_count {
        return Ok(ExportPage {
            records,
            next_cursor: None,
            total: total_count,
            has_more: false,
        });
    }

    let index = get_index(env);
    let end = (cursor.saturating_add(effective_limit)).min(index.len());

    for i in cursor..end {
        if let Some(username) = index.get(i) {
            if let Some(record) = get_record(env, &username) {
                records.push_back((username, record));
            }
        }
    }

    let next_cursor = if end < index.len() { Some(end) } else { None };
    let has_more = next_cursor.is_some();

    Ok(ExportPage {
        records,
        next_cursor,
        total: total_count,
        has_more,
    })
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

/// True when `address` is the contract admin.
pub fn is_admin_caller(env: &Env, address: &Address) -> bool {
    matches!(get_admin(env), Ok(admin) if admin == *address)
}

/// Timestamp of `github_username`'s last cooldown-tracked action, or 0 if it
/// has none. Cooldown is tracked per username rather than globally so one
/// contributor's activity cannot block everyone else's.
pub fn get_last_action(env: &Env, github_username: &String) -> u64 {
    env.storage()
        .persistent()
        .get(&(LAST_ACT_KEY, github_username.clone()))
        .unwrap_or(0)
}

pub fn set_last_action(env: &Env, github_username: &String, timestamp: u64) {
    let key = (LAST_ACT_KEY, github_username.clone());
    env.storage().persistent().set(&key, &timestamp);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

/// True when the configured cooldown has not yet elapsed since
/// `github_username`'s last tracked action. A cooldown of 0 disables the
/// check entirely.
pub fn is_in_cooldown(env: &Env, github_username: &String) -> bool {
    let cooldown = get_cooldown(env);
    if cooldown == 0 {
        return false;
    }
    let last = get_last_action(env, github_username);
    if last == 0 {
        return false;
    }
    env.ledger().timestamp() < last.saturating_add(cooldown)
}

#[allow(dead_code)] // Staged for role-gated entry points; covered by role tests.
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
