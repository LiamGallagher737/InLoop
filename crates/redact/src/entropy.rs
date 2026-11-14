//! Small helpers shared by the layers.

/// Shannon entropy of `bytes`, in bits per byte.
pub(crate) fn shannon(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in bytes {
        counts[usize::from(b)] += 1;
    }
    let len = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// True if the name of a variable or field suggests its value is a secret.
pub(crate) fn name_looks_secret(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const NOT_SECRET: &[&str] = &[
        "SSH_AUTH_SOCK",
        "XAUTHORITY",
        "KEYTIMEOUT",
        "GPG_AGENT_INFO",
        "KEYMAP",
        "KEYBOARD",
    ];
    if NOT_SECRET.contains(&upper.as_str()) || upper.contains("KEYCHAIN") {
        return false;
    }
    const NOT_SECRET_SUFFIXES: &[&str] = &[
        "_PATH",
        "_FILE",
        "_DIR",
        "_SOCK",
        "_SOCKET",
        "_HOME",
        "_TTY",
        "_URL_PATH",
        "_TIMEOUT",
        "_TTL",
        "_ENABLED",
        "_LENGTH",
        "_SIZE",
        "_COUNT",
        "_NAME",
        "_TYPE",
        "_ALGORITHM",
    ];
    if NOT_SECRET_SUFFIXES.iter().any(|s| upper.ends_with(s)) {
        return false;
    }
    const SECRET_PARTS: &[&str] = &[
        "KEY",
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "PASSPHRASE",
        "CREDENTIAL",
        "AUTH",
        "PRIVATE",
        "DSN",
        "CONNECTION_STRING",
        "COOKIE",
        "SIGNATURE",
        "SALT",
    ];
    if SECRET_PARTS.iter().any(|p| upper.contains(p)) {
        return true;
    }
    if upper == "PWD" || upper == "OLDPWD" {
        return false;
    }
    upper.ends_with("PASS")
        || upper.ends_with("PWD")
        || upper
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| matches!(part, "PASS" | "PWD" | "PW" | "PIN" | "CREDS"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn entropy_of_uniform_text() {
        assert_eq!(shannon(b""), 0.0);
        assert_eq!(shannon(b"aaaa"), 0.0);
        assert!((shannon(b"abcd") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn secret_names() {
        for n in [
            "OPENAI_API_KEY",
            "db_password",
            "GITHUB_TOKEN",
            "SENTRY_DSN",
            "PGPASS",
            "MYSQL_PWD",
            "AWS_SECRET_ACCESS_KEY",
        ] {
            assert!(name_looks_secret(n), "{n}");
        }
        for n in [
            "PATH",
            "SSH_AUTH_SOCK",
            "PWD",
            "OLDPWD",
            "HOME",
            "LOGIN_KEYCHAIN_PATH",
            "KEYTIMEOUT",
            "GOOGLE_APPLICATION_CREDENTIALS_FILE",
            "TERM",
            "XAUTHORITY",
            "CLAUDE_SESSION_ID",
            "TERM_SESSION_ID",
            "ITERM_SESSION_ID",
            "SECURITYSESSIONID",
            "INLOOP_SESSION_ID",
        ] {
            assert!(!name_looks_secret(n), "{n}");
        }
    }
}
