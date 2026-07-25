//! Input validation helpers for TrustBridge contract operations.
//!
//! Validation runs before authentication and before any storage write, so a
//! malformed username is rejected at the cheapest possible point. Everything
//! here works on a fixed stack buffer: the contract is `#![no_std]` and must
//! not allocate on the validation path.

use soroban_sdk::String;

/// GitHub caps usernames at 39 characters.
pub const MAX_USERNAME_LEN: u32 = 39;

/// Copies a username into a fixed stack buffer.
///
/// Returns `None` when the string is empty or longer than a GitHub username
/// can be, which also bounds the buffer copy below.
fn copy_username(s: &String) -> Option<([u8; MAX_USERNAME_LEN as usize], usize)> {
    let len = s.len();
    if len == 0 || len > MAX_USERNAME_LEN {
        return None;
    }

    let mut buf = [0u8; MAX_USERNAME_LEN as usize];
    s.copy_into_slice(&mut buf[..len as usize]);

    Some((buf, len as usize))
}

/// Check whether a string has no content.
pub fn is_empty(s: &String) -> bool {
    s.len() == 0
}

/// Validate that a GitHub username follows basic rules.
///
/// Accepts 1 to 39 characters of alphanumerics, hyphens, and underscores, with
/// an alphanumeric first and last character. Underscores are not valid on
/// GitHub itself but are accepted here so registrations made before validation
/// existed remain readable and removable.
pub fn is_valid_github_username(s: &String) -> bool {
    let Some((buf, len)) = copy_username(s) else {
        return false;
    };
    let bytes = &buf[..len];

    if !bytes[0].is_ascii_alphanumeric() || !bytes[len - 1].is_ascii_alphanumeric() {
        return false;
    }

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

    match (copy_username(a), copy_username(b)) {
        (Some((left, len)), Some((right, _))) => left[..len].eq_ignore_ascii_case(&right[..len]),
        _ => false,
    }
}

/// Calculate the percentage of verified contributors out of total.
pub fn calculate_verification_percentage(verified: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    ((verified as u64 * 100) / (total as u64)) as u32
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
    fn test_valid_github_usernames_are_accepted() {
        let env = Env::default();

        assert!(is_valid_github_username(&s(&env, "a")));
        assert!(is_valid_github_username(&s(&env, "alice")));
        assert!(is_valid_github_username(&s(&env, "bob-smith")));
        assert!(is_valid_github_username(&s(&env, "user_123")));
        assert!(is_valid_github_username(&s(&env, "Octocat")));
        assert!(is_valid_github_username(&s(&env, &"a".repeat(39))));
    }

    #[test]
    fn test_invalid_github_usernames_are_rejected() {
        let env = Env::default();

        assert!(!is_valid_github_username(&s(&env, "")));
        assert!(!is_valid_github_username(&s(&env, " ")));
        assert!(!is_valid_github_username(&s(&env, "-invalid")));
        assert!(!is_valid_github_username(&s(&env, "invalid-")));
        assert!(!is_valid_github_username(&s(&env, "_invalid")));
        assert!(!is_valid_github_username(&s(&env, "a@invalid")));
        assert!(!is_valid_github_username(&s(&env, "spaced name")));
        assert!(!is_valid_github_username(&s(&env, "new\nline")));
        assert!(!is_valid_github_username(&s(&env, &"a".repeat(40))));
    }

    #[test]
    fn test_eq_ignore_ascii_case() {
        let env = Env::default();

        assert!(eq_ignore_ascii_case(&s(&env, "Octocat"), &s(&env, "octocat")));
        assert!(eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "ALICE")));
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "bob")));
        assert!(!eq_ignore_ascii_case(&s(&env, "alice"), &s(&env, "alice2")));
        // Out-of-range inputs compare false rather than panicking on copy.
        assert!(!eq_ignore_ascii_case(
            &s(&env, &"a".repeat(40)),
            &s(&env, &"a".repeat(40))
        ));
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
