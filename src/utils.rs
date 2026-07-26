//! Input validation helpers for TrustBridge contract operations.
//!
//! Validation runs before authentication and before any storage write, so a
//! malformed username is rejected at the cheapest possible point. Everything
//! here works on a fixed stack buffer: the contract is `#![no_std]` and must
//! not allocate on the validation path.

// Some helpers here are staged ahead of their call sites: they are covered by
// this module's own tests but are not yet wired into `lib.rs`.
#![allow(dead_code)]

use soroban_sdk::{Env, String};

/// GitHub caps usernames at 39 characters.
pub const MAX_USERNAME_LEN: u32 = 39;

/// Stack buffer size for username copies. Sized above `MAX_USERNAME_LEN`, so
/// an over-long username is rejected on length before it is ever read.
const USERNAME_BUF: usize = 64;

/// Copies a username into a fixed stack buffer.
///
/// Returns `None` when the username is empty or does not fit, which callers
/// treat as a validation failure rather than truncating.
fn copy_into_buf(s: &String, buf: &mut [u8; USERNAME_BUF]) -> Option<usize> {
    let len = s.len() as usize;
    if len == 0 || len > USERNAME_BUF {
        return None;
    }
    s.copy_into_slice(&mut buf[..len]);
    Some(len)
}

/// Check if a string is empty.
pub fn is_empty(s: &String) -> bool {
    s.is_empty()
}

/// Check if a string is empty or contains only ASCII whitespace.
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
///
/// Accepts 1 to 39 characters of alphanumerics, hyphens, and underscores, with
/// an alphanumeric first and last character. Underscores are not valid on
/// GitHub itself but are accepted here so registrations made before validation
/// existed remain readable and removable.
pub fn is_valid_github_username(s: &String) -> bool {
    if s.len() > MAX_USERNAME_LEN {
        return false;
    }

    let mut buf = [0u8; USERNAME_BUF];
    let len = match copy_into_buf(s, &mut buf) {
        Some(len) => len,
        None => return false,
    };
    let bytes = &buf[..len];

    // First and last characters must be alphanumeric.
    if !bytes[0].is_ascii_alphanumeric() || !bytes[len - 1].is_ascii_alphanumeric() {
        return false;
    }

    // All characters must be alphanumeric, hyphen, or underscore.
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

/// Case-insensitive comparison of two usernames.
///
/// GitHub usernames are case-insensitive, so this is what an off-chain
/// verification workflow should use when matching a registration against a
/// GitHub identity.
pub fn eq_ignore_ascii_case(a: &String, b: &String) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut buf_a = [0u8; USERNAME_BUF];
    let mut buf_b = [0u8; USERNAME_BUF];
    let (len_a, len_b) = match (copy_into_buf(a, &mut buf_a), copy_into_buf(b, &mut buf_b)) {
        (Some(la), Some(lb)) => (la, lb),
        // Equal lengths that failed to buffer means both are empty (equal) or
        // both are too long to compare on the stack (reported unequal).
        _ => return a.is_empty(),
    };

    buf_a[..len_a]
        .iter()
        .zip(buf_b[..len_b].iter())
        .all(|(x, y)| x.eq_ignore_ascii_case(y))
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
    use soroban_sdk::Env;

    fn s(env: &Env, value: &str) -> String {
        String::from_str(env, value)
    }

    #[test]
    fn test_is_empty() {
        let env = Env::default();
        assert!(is_empty(&s(&env, "")));
        assert!(!is_empty(&s(&env, " ")));
        assert!(!is_empty(&s(&env, "alice")));
    }

    #[test]
    fn test_is_empty_or_whitespace() {
        let env = Env::default();

        assert!(is_empty_or_whitespace(&s(&env, "")));
        assert!(is_empty_or_whitespace(&s(&env, "   ")));
        assert!(!is_empty_or_whitespace(&s(&env, "hello")));
    }

    #[test]
    fn test_is_valid_github_username() {
        let env = Env::default();

        assert!(is_valid_github_username(&s(&env, "alice")));
        assert!(is_valid_github_username(&s(&env, "bob-smith")));
        assert!(is_valid_github_username(&s(&env, "user_123")));
        assert!(!is_valid_github_username(&s(&env, "-invalid")));
        assert!(!is_valid_github_username(&s(&env, "invalid-")));
        assert!(!is_valid_github_username(&s(&env, "a@invalid")));
        assert!(!is_valid_github_username(&s(&env, "")));
    }

    #[test]
    fn test_eq_ignore_ascii_case() {
        let env = Env::default();

        assert!(eq_ignore_ascii_case(&s(&env, "Alice"), &s(&env, "alice")));
        assert!(eq_ignore_ascii_case(&s(&env, "BOB-1"), &s(&env, "bob-1")));
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "bob")));
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "alice1")));
    }

    #[test]
    fn test_calculate_verification_percentage() {
        assert_eq!(calculate_verification_percentage(0, 100), 0);
        assert_eq!(calculate_verification_percentage(50, 100), 50);
        assert_eq!(calculate_verification_percentage(100, 100), 100);
        assert_eq!(calculate_verification_percentage(1, 3), 33);
        assert_eq!(calculate_verification_percentage(10, 0), 0);
    }

    #[test]
    fn test_percentage_does_not_overflow_at_u32_max() {
        assert_eq!(calculate_verification_percentage(u32::MAX, u32::MAX), 100);
    }
}
