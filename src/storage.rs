use soroban_sdk::{symbol_short, Address, BytesN, Env, String, Symbol, Vec};

use crate::ContractError;

// ── Storage keys ────────────────────────────────────────────────────────────

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
pub const CHUNK_CNT_KEY: Symbol = symbol_short!("chkcnt");
pub const LAST_ACT_KEY: Symbol = symbol_short!("lastact");
pub const PROV_KEY: Symbol = symbol_short!("prov");
pub const ATTEST_KEY: Symbol = symbol_short!("attest");

/// Key for the version stored at `storage::get_version` / `set_version`.
/// Aliased as VERSION_KEY for callers that use that name.
pub const VERSION_KEY: Symbol = VER_KEY;

// ── TTL constants (ledger-based, ~7 days at 5 s/ledger) ─────────────────────

/// Ledgers per day at the ~5s close time, used to express the policy in days.
pub const LEDGERS_PER_DAY: u32 = 17_280;

/// Only extend when fewer than this many ledgers remain (~30 days).
///
/// `extend_ttl` is a no-op when the remaining TTL already exceeds the
/// threshold, so this is what keeps a hot record from paying the extension
/// cost on every single read.
pub const TTL_THRESHOLD: u32 = LEDGERS_PER_DAY * 30;

/// Extend to this many ledgers from the current one (~90 days).
///
/// Comfortably inside the network's maximum persistent TTL, so an extension is
/// never rejected for overshooting the cap.
pub const TTL_BUMP: u32 = LEDGERS_PER_DAY * 90;

// ── Pagination constants ─────────────────────────────────────────────────────

/// Page size used when a caller passes `limit = 0`.
pub const DEFAULT_PAGE_LIMIT: u32 = 20;
/// Upper bound on a single export page, to keep the response under the
/// transaction result size limit.
pub const MAX_PAGE_LIMIT: u32 = 100;

// ── Chunked-index constants ──────────────────────────────────────────────────

/// Maximum number of usernames per chunk slice.
pub const CHUNK_SIZE: u32 = 50;

// ── Username validation ──────────────────────────────────────────────────────

/// Stack buffer length for username case-normalization comparisons.
pub const USERNAME_BUF_LEN: u32 = 64;

// ── Types ────────────────────────────────────────────────────────────────────

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

/// Provenance of the currently deployed WASM executable (Wave #24).
///
/// `upgrade` previously left no queryable trace of what it did — it wrote a
/// bare timestamp to `LAST_UPG_KEY` and published an event. Events are not
/// contract state: an auditor asking "what is deployed right now, and what did
/// it replace?" had to reconstruct the answer by replaying the whole event
/// history, and could not do it from a contract call at all.
///
/// This is the answer as a single readable record. `previous_wasm_hash` is what
/// makes it a chain rather than a snapshot: each record names its predecessor,
/// so the lineage can be walked backwards through historical `UpgradedEvent`s
/// even though only the head is stored.
#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
pub struct WasmProvenance {
    /// Hash of the WASM currently executing.
    pub wasm_hash: BytesN<32>,
    /// Hash this one replaced. `None` for the first upgrade after deployment.
    pub previous_wasm_hash: Option<BytesN<32>>,
    /// Address that authorised the upgrade.
    pub upgraded_by: Address,
    /// Ledger timestamp the upgrade was applied.
    pub upgraded_at: u64,
    /// Contract version recorded at upgrade time.
    pub version: (u32, u32, u32),
    /// Whether the hash had been attested before it was applied.
    pub attested: bool,
}

/// An admin's advance declaration of the WASM hash they intend to deploy.
///
/// Optional two-step upgrade. When an attestation is live, `upgrade` will only
/// accept the hash it names — so a compromised admin key cannot swap in a
/// different binary at the moment of the upgrade without first publishing that
/// intent, on-chain, ahead of time.
///
/// The expiry is the point: an attestation that never lapsed would be a
/// standing authorisation for that hash, which is strictly worse than none.
#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype]
pub struct WasmAttestation {
    /// Hash the admin has declared they intend to deploy.
    pub wasm_hash: BytesN<32>,
    /// Ledger timestamp after which this attestation is no longer valid.
    pub expires_at: u64,
    /// Address that published the attestation.
    pub attested_by: Address,
    /// Ledger timestamp the attestation was published.
    pub attested_at: u64,
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

// ── Initialization / admin ───────────────────────────────────────────────────

pub fn require_initialized(env: &Env) -> Result<(), ContractError> {
    if env.storage().instance().has(&ADMIN_KEY) {
        Ok(())
    } else {
        Err(ContractError::NotInitialized)
    }
}

pub fn require_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        Err(ContractError::Paused)
    } else {
        Ok(())
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

/// Extends a single record's TTL without deserialising it (Wave #7).
///
/// `get_record` also extends as a side effect of reading, but it pays to decode
/// the `ContributorRecord` first. A keeper bumping thousands of entries does not
/// want the value, only the extension — this skips that cost.
///
/// Returns whether the entry existed. A missing entry is not an error: the
/// keeper's list is built off-chain and can lag behind removals.
pub fn extend_record_ttl(env: &Env, github_username: &String) -> bool {
    let key = (REG_KEY, github_username.clone());
    if !env.storage().persistent().has(&key) {
        return false;
    }
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
    true
}

pub fn remove_record(env: &Env, github_username: &String) {
    let key = (REG_KEY, github_username.clone());
    env.storage().persistent().remove(&key);
}

pub fn has_record(env: &Env, github_username: &String) -> bool {
    get_record(env, github_username).is_some()
}

// ── Counters ─────────────────────────────────────────────────────────────────

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

// ── Chunked username index ───────────────────────────────────────────────────

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

// ── Stats ────────────────────────────────────────────────────────────────────

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

// ── Cooldown / upgrade timelock ───────────────────────────────────────────────

pub fn get_cooldown(env: &Env) -> u64 {
    env.storage().instance().get(&COOLDOWN_KEY).unwrap_or(0)
}

pub fn set_cooldown(env: &Env, cooldown_seconds: u64) {
    env.storage()
        .instance()
        .set(&COOLDOWN_KEY, &cooldown_seconds);
}

// ─── WASM provenance & attestation (Wave #24) ────────────────────────────────

/// Provenance of the currently deployed WASM. `None` before the first upgrade.
pub fn get_wasm_provenance(env: &Env) -> Option<WasmProvenance> {
    env.storage().instance().get(&PROV_KEY)
}

pub fn set_wasm_provenance(env: &Env, provenance: &WasmProvenance) {
    env.storage().instance().set(&PROV_KEY, provenance);
}

/// The pending upgrade attestation, if one has been published.
///
/// Returns the raw record regardless of expiry — callers decide what to do with
/// a lapsed attestation, and `get_wasm_attestation` is also a read endpoint
/// where seeing the expired value is useful for diagnosis.
pub fn get_wasm_attestation(env: &Env) -> Option<WasmAttestation> {
    env.storage().instance().get(&ATTEST_KEY)
}

pub fn set_wasm_attestation(env: &Env, attestation: &WasmAttestation) {
    env.storage().instance().set(&ATTEST_KEY, attestation);
}

pub fn remove_wasm_attestation(env: &Env) {
    env.storage().instance().remove(&ATTEST_KEY);
}

pub fn get_last_upgrade(env: &Env) -> u64 {
    env.storage().instance().get(&LAST_UPG_KEY).unwrap_or(0)
}

pub fn set_last_upgrade(env: &Env, timestamp: u64) {
    env.storage().instance().set(&LAST_UPG_KEY, &timestamp);
}

// ── Per-user action cooldown (Wave #33) ──────────────────────────────────────

/// Records the ledger timestamp of the last mutating action for `github_username`.
pub fn set_last_action(env: &Env, github_username: &String, timestamp: u64) {
    let key = (LAST_ACT_KEY, github_username.clone());
    env.storage().persistent().set(&key, &timestamp);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

/// Returns the timestamp of the last recorded action for `github_username`, or 0.
pub fn get_last_action(env: &Env, github_username: &String) -> u64 {
    let key = (LAST_ACT_KEY, github_username.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Returns true if `github_username` is still within the WASM-upgrade cooldown
/// window for per-user rate-limiting.
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

// ── Role-based access control ─────────────────────────────────────────────────

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

/// True when `address` is the contract admin or holds `expected_role`.
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

// ── Pause state ──────────────────────────────────────────────────────────────

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&PAUSED_KEY).unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&PAUSED_KEY, &paused);
}

// ── Version ──────────────────────────────────────────────────────────────────

pub fn get_version(env: &Env) -> Option<(u32, u32, u32)> {
    env.storage().instance().get(&VER_KEY)
}

pub fn set_version(env: &Env, version: (u32, u32, u32)) {
    env.storage().instance().set(&VER_KEY, &version);
}
