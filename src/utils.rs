//! Input validation helpers for TrustBridge contract operations.
//!
//! Validation runs before authentication and before any storage write, so a
//! malformed username is rejected at the cheapest possible point. Everything
//! here works on a fixed stack buffer: the contract is `#![no_std]` and must
//! not allocate on the validation path.

use soroban_sdk::{Env, String};

/// GitHub caps usernames at 39 characters.
pub const MAX_USERNAME_LEN: u32 = 39;

/// Stack buffer for username inspection.
///
/// Sized above `MAX_USERNAME_LEN` so the length check can reject over-long
/// input before any copy happens — `copy_into_slice` panics on a length
/// mismatch, and a panic in a contract is a failed invocation with no usable
/// error code, so the guard must come first.
const USERNAME_BUF_LEN: usize = 64;

/// Check if a string is empty.
pub fn is_empty(s: &String) -> bool {
    s.len() == 0
}

/// Check if a string is empty or contains only ASCII whitespace.
pub fn is_empty_or_whitespace(s: &String) -> bool {
    let len = s.len() as usize;
    if len == 0 {
        return true;
    }
    if len > USERNAME_BUF_LEN {
        // Too long to inspect on the stack, but definitively not whitespace-only
        // for any input this contract accepts.
        return false;
    }
    let mut buf = [0u8; USERNAME_BUF_LEN];
    s.copy_into_slice(&mut buf[..len]);
    buf[..len].iter().all(|b| b.is_ascii_whitespace())
}

/// Validate that a GitHub username follows GitHub's own rules.
///
/// Accepted:
/// - 1 to 39 characters (`MAX_USERNAME_LEN`)
/// - ASCII alphanumerics, hyphens, and underscores only
/// - first and last character alphanumeric
/// - no consecutive hyphens
///
/// Underscores are not valid on GitHub itself but are accepted here so that
/// registrations made before validation existed remain readable and removable.
/// Rejecting them would strand those records: `remove` looks the username up by
/// exact key, so a name that cannot be expressed can never be cleaned up.
///
/// The comparison is byte-wise ASCII. `String::len()` returns a byte count, so
/// any multi-byte UTF-8 sequence fails the alphanumeric check on its leading
/// byte and is rejected — which is correct, since GitHub usernames are ASCII.
pub fn is_valid_github_username(s: &String) -> bool {
    let len = s.len() as usize;

    // Length check first: this is what makes the copy below safe.
    if len < 1 || len > MAX_USERNAME_LEN as usize {
        return false;
    }

    let mut buf = [0u8; USERNAME_BUF_LEN];
    s.copy_into_slice(&mut buf[..len]);
    let bytes = &buf[..len];

    // First and last characters must be alphanumeric. `len >= 1` is guaranteed
    // above, so indexing is safe; for a 1-character name both checks read the
    // same byte.
    if !bytes[0].is_ascii_alphanumeric() || !bytes[len - 1].is_ascii_alphanumeric() {
        return false;
    }

    // Every character must be alphanumeric, hyphen, or underscore, and hyphens
    // may not repeat — GitHub rejects "foo--bar".
    let mut prev_hyphen = false;
    for b in bytes {
        let is_hyphen = *b == b'-';
        if is_hyphen && prev_hyphen {
            return false;
        }
        if !b.is_ascii_alphanumeric() && !is_hyphen && *b != b'_' {
            return false;
        }
        prev_hyphen = is_hyphen;
    }

    true
}

/// Case-insensitive comparison of two usernames.
///
/// GitHub usernames are case-insensitive, so this is what an off-chain
/// verification workflow should use when matching a registration against a
/// GitHub identity. Note that storage keys are still case-*sensitive*: this
/// compares two values, it does not normalise them.
pub fn eq_ignore_ascii_case(a: &String, b: &String) -> bool {
    let len = a.len() as usize;
    if len != b.len() as usize {
        return false;
    }
    if len == 0 {
        return true;
    }
    if len > USERNAME_BUF_LEN {
        return false;
    }

    let mut buf_a = [0u8; USERNAME_BUF_LEN];
    let mut buf_b = [0u8; USERNAME_BUF_LEN];
    a.copy_into_slice(&mut buf_a[..len]);
    b.copy_into_slice(&mut buf_b[..len]);

    buf_a[..len]
        .iter()
        .zip(buf_b[..len].iter())
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
    fn test_accepts_valid_usernames() {
        let env = Env::default();

        assert!(is_valid_github_username(&s(&env, "alice")));
        assert!(is_valid_github_username(&s(&env, "bob-smith")));
        assert!(is_valid_github_username(&s(&env, "user_123")));
        // Single character, and digits at both ends.
        assert!(is_valid_github_username(&s(&env, "a")));
        assert!(is_valid_github_username(&s(&env, "7")));
        // Exactly at the 39-character limit.
        assert!(is_valid_github_username(&s(
            &env,
            "abcdefghijabcdefghijabcdefghijabcdefghi"
        )));
    }

    #[test]
    fn test_rejects_bad_boundary_characters() {
        let env = Env::default();

        assert!(!is_valid_github_username(&s(&env, "-invalid")));
        // Trailing hyphen: the check the previous implementation was missing.
        assert!(!is_valid_github_username(&s(&env, "invalid-")));
        assert!(!is_valid_github_username(&s(&env, "_leading")));
        assert!(!is_valid_github_username(&s(&env, "trailing_")));
    }

    #[test]
    fn test_rejects_illegal_characters() {
        let env = Env::default();

        assert!(!is_valid_github_username(&s(&env, "a@invalid")));
        assert!(!is_valid_github_username(&s(&env, "has space")));
        assert!(!is_valid_github_username(&s(&env, "dot.name")));
        assert!(!is_valid_github_username(&s(&env, "slash/name")));
    }

    #[test]
    fn test_rejects_consecutive_hyphens() {
        let env = Env::default();

        assert!(!is_valid_github_username(&s(&env, "foo--bar")));
        assert!(is_valid_github_username(&s(&env, "foo-bar-baz")));
    }

    #[test]
    fn test_rejects_out_of_range_lengths() {
        let env = Env::default();

        assert!(!is_valid_github_username(&s(&env, "")));
        // 40 characters — one past MAX_USERNAME_LEN.
        assert!(!is_valid_github_username(&s(
            &env,
            "abcdefghijabcdefghijabcdefghijabcdefghij"
        )));
    }

    #[test]
    fn test_rejects_non_ascii() {
        let env = Env::default();

        // Multi-byte UTF-8 fails on its leading byte, which is what we want:
        // GitHub usernames are ASCII.
        assert!(!is_valid_github_username(&s(&env, "café")));
        assert!(!is_valid_github_username(&s(&env, "日本語")));
    }

    #[test]
    fn test_eq_ignore_ascii_case() {
        let env = Env::default();

        assert!(eq_ignore_ascii_case(&s(&env, "Alice"), &s(&env, "alice")));
        assert!(eq_ignore_ascii_case(&s(&env, "BOB-SMITH"), &s(&env, "bob-smith")));
        assert!(eq_ignore_ascii_case(&s(&env, ""), &s(&env, "")));
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "bob")));
        // Differing lengths short-circuit.
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "alice2")));
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
        // The u64 widening is what keeps this from wrapping.
        assert_eq!(calculate_verification_percentage(u32::MAX, u32::MAX), 100);
    }
}
