/// Utility functions for TrustBridge contract operations.
///
/// This module provides helper functions for common contract operations,
/// string manipulation, and validation.
use soroban_sdk::{Env, String};

/// Check if a string is empty or contains only whitespace.
pub fn is_empty_or_whitespace(s: &String) -> bool {
    let len = s.len() as usize;
    if len == 0 {
        return true;
    }
    let mut buf = [0u8; 128];
    let slice_len = len.min(128);
    s.copy_into_slice(&mut buf[..slice_len]);
    buf[..slice_len].iter().all(|b| b.is_ascii_whitespace())
}

/// Validate that a GitHub username follows basic rules.
/// GitHub usernames must be 1-39 characters, alphanumeric with hyphens/underscores.
pub fn is_valid_github_username(s: &String) -> bool {
    let len = s.len() as usize;

    // Length check: 1-39 characters
    if len < 1 || len > 39 {
        return false;
    }

    let mut buf = [0u8; 64];
    s.copy_into_slice(&mut buf[..len]);
    let bytes = &buf[..len];

    // First character must be alphanumeric
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }

    // Last character must be alphanumeric
    if !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return false;
    }

    // All characters must be alphanumeric, hyphen, or underscore
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

/// Calculate the percentage of verified contributors out of total.
pub fn calculate_verification_percentage(verified: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    ((verified as u64 * 100) / (total as u64)) as u32
}

/// Generate a timestamped event ID for audit trails.
pub fn generate_event_id(env: &Env, nonce: u32) -> u64 {
    let timestamp = env.ledger().timestamp();
    (timestamp << 32) | (nonce as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_empty_or_whitespace() {
        let env = Env::default();
        let empty = String::from_str(&env, "");
        let whitespace = String::from_str(&env, "   ");
        let valid = String::from_str(&env, "hello");

        assert!(is_empty_or_whitespace(&empty));
        assert!(is_empty_or_whitespace(&whitespace));
        assert!(!is_empty_or_whitespace(&valid));
    }

    #[test]
    fn test_is_valid_github_username() {
        let env = Env::default();
        let valid1 = String::from_str(&env, "alice");
        let valid2 = String::from_str(&env, "bob-smith");
        let valid3 = String::from_str(&env, "user_123");
        let invalid1 = String::from_str(&env, "-invalid");
        let invalid2 = String::from_str(&env, "invalid-");
        let invalid3 = String::from_str(&env, "a@invalid");

        assert!(is_valid_github_username(&valid1));
        assert!(is_valid_github_username(&valid2));
        assert!(is_valid_github_username(&valid3));
        assert!(!is_valid_github_username(&invalid1));
        assert!(!is_valid_github_username(&invalid2));
        assert!(!is_valid_github_username(&invalid3));
    }

    #[test]
    fn test_calculate_verification_percentage() {
        assert_eq!(calculate_verification_percentage(0, 100), 0);
        assert_eq!(calculate_verification_percentage(50, 100), 50);
        assert_eq!(calculate_verification_percentage(100, 100), 100);
        assert_eq!(calculate_verification_percentage(1, 3), 33);
        assert_eq!(calculate_verification_percentage(10, 0), 0);
    }
}
