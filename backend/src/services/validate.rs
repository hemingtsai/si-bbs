//! Pure input validation for the unauthenticated auth endpoints.
//!
//! These functions only decide *shape*; they never touch the database or hash
//! anything, so they are cheap to run before any expensive work and trivial to
//! unit-test. Messages are English because the rest of the API's errors are.

pub const USERNAME_MIN_CHARS: usize = 3;
pub const USERNAME_MAX_CHARS: usize = 32;
pub const EMAIL_MAX_CHARS: usize = 254;
pub const PASSWORD_MIN_CHARS: usize = 6;
pub const PASSWORD_MAX_CHARS: usize = 128;

/// Validate a username and return it trimmed.
///
/// Lengths are counted in `char`s, not bytes: a three-character Chinese name is
/// perfectly reasonable but nine bytes long.
pub fn username(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    let len = value.chars().count();
    if !(USERNAME_MIN_CHARS..=USERNAME_MAX_CHARS).contains(&len) {
        return Err(format!(
            "username must be between {USERNAME_MIN_CHARS} and {USERNAME_MAX_CHARS} characters"
        ));
    }
    // Letters/digits in any script (so CJK names work), plus the two separators.
    // Whitespace, punctuation and control characters are refused, which keeps
    // display and mentions unambiguous.
    if !value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(
            "username may only contain letters, digits, underscores and hyphens".to_string(),
        );
    }
    Ok(value.to_string())
}

/// Validate an email address and return it trimmed.
///
/// Deliberately structural rather than RFC-complete: one `@`, a non-empty local
/// part, and a dotted domain. Deliverability can only be proven by sending mail,
/// which this project does not do.
pub fn email(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.chars().count() > EMAIL_MAX_CHARS {
        return Err(format!(
            "email must be at most {EMAIL_MAX_CHARS} characters"
        ));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("email must not contain whitespace".to_string());
    }

    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("email must contain exactly one @".to_string());
    };
    if local.is_empty() || local.chars().count() > 64 {
        return Err("email local part is invalid".to_string());
    }
    if domain.len() < 4
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.starts_with('-')
        || domain.ends_with('-')
        || domain.contains("..")
    {
        return Err("email domain is invalid".to_string());
    }

    Ok(value.to_string())
}

/// Validate a password. Only bounds are enforced here; strength beyond length is
/// a product decision (`PASSWORD_MIN_CHARS` is deliberately low and documented).
pub fn password(raw: &str) -> Result<(), String> {
    let len = raw.chars().count();
    if len < PASSWORD_MIN_CHARS {
        return Err(format!(
            "password must be at least {PASSWORD_MIN_CHARS} characters"
        ));
    }
    // Without an upper bound an unauthenticated caller can hand Argon2 megabytes
    // of input and burn CPU on every attempt.
    if len > PASSWORD_MAX_CHARS {
        return Err(format!(
            "password must be at most {PASSWORD_MAX_CHARS} characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_accepts_expected_shapes() {
        assert_eq!(username("  alice  ").unwrap(), "alice");
        assert_eq!(username("a_b-c").unwrap(), "a_b-c");
        assert_eq!(username("用户一号").unwrap(), "用户一号");
    }

    #[test]
    fn username_bounds_are_measured_in_chars() {
        assert!(username(&"a".repeat(USERNAME_MIN_CHARS - 1)).is_err());
        assert!(username(&"a".repeat(USERNAME_MIN_CHARS)).is_ok());
        assert!(username(&"a".repeat(USERNAME_MAX_CHARS)).is_ok());
        assert!(username(&"a".repeat(USERNAME_MAX_CHARS + 1)).is_err());
        // Three CJK characters are nine bytes but still valid.
        assert!(username("用户名").is_ok());
    }

    #[test]
    fn username_rejects_separators_and_whitespace() {
        for bad in ["a b", "a@b", "a.b", "a/b", "a\nb", "<script>"] {
            assert!(username(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn email_accepts_common_addresses() {
        assert_eq!(email(" Alice@Example.com ").unwrap(), "Alice@Example.com");
        assert!(email("a@b.co").is_ok());
        assert!(email("first.last+tag@sub.example.com").is_ok());
    }

    #[test]
    fn email_rejects_malformed_addresses() {
        for bad in [
            "not-an-email",
            "@example.com",
            "a@",
            "a@b",
            "a@@b.com",
            "a@.com",
            "a@b..com",
            "a@b.com.",
            "a b@example.com",
        ] {
            assert!(email(bad).is_err(), "{bad} should be rejected");
        }
        let too_long = format!("{}@example.com", "a".repeat(EMAIL_MAX_CHARS));
        assert!(email(&too_long).is_err());
    }

    #[test]
    fn password_bounds_are_measured_in_chars() {
        assert!(password(&"a".repeat(PASSWORD_MIN_CHARS - 1)).is_err());
        assert!(password(&"a".repeat(PASSWORD_MIN_CHARS)).is_ok());
        assert!(password(&"a".repeat(PASSWORD_MAX_CHARS)).is_ok());
        assert!(password(&"a".repeat(PASSWORD_MAX_CHARS + 1)).is_err());
        // Six CJK characters are eighteen bytes: the old byte-based check let two
        // emoji through and this one still accepts a legitimately short password.
        assert!(password("密码密码密码").is_ok());
        assert!(password("密码密码").is_err());
    }
}
