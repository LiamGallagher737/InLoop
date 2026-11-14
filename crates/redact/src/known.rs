//! Layer 1: known values.
//!
//! The real secret values InLoop can find (`.env`-style files and
//! secret-looking environment variables) are matched exactly, along with their
//! base64, URL-encoded and JSON-escaped forms. Values are held in
//! `SecretString` until they go into the matcher and are never logged or
//! printed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use secrecy::{ExposeSecret, SecretString};

use crate::entropy::name_looks_secret;
use crate::{PRIORITY_KNOWN, RedactError, RedactorConfig, Span};

pub(crate) const KIND: &str = "known-value";

/// Values shorter than this are ignored: too likely to occur by chance.
const MIN_VALUE_LEN: usize = 8;
/// Encoded fragments shorter than this are not matched on their own.
const MIN_FRAGMENT_LEN: usize = 10;
/// How deep under the root to look for known-value files.
const MAX_DEPTH: usize = 6;
/// Directories never searched for known-value files.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    "dist",
    "build",
    ".cache",
    ".turbo",
];

pub(crate) struct KnownValues {
    matcher: Option<AhoCorasick>,
    count: usize,
}

impl KnownValues {
    pub(crate) fn load(config: &RedactorConfig) -> Result<KnownValues, RedactError> {
        let mut values: Vec<SecretString> = Vec::new();
        for path in matching_files(&config.root, &config.known_value_files)? {
            let text = match std::fs::read(&path) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(RedactError::Io { path, source }),
            };
            let text = SecretString::from(text);
            for (key, value) in parse_dotenv(text.expose_secret()) {
                if keep_value(&key, value.expose_secret()) {
                    values.push(value);
                }
            }
        }
        if config.include_process_env {
            let env = std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)));
            values.extend(env_values(env));
        }
        Self::from_values(values)
    }

    pub(crate) fn from_values(values: Vec<SecretString>) -> Result<KnownValues, RedactError> {
        let mut distinct: BTreeSet<&str> = BTreeSet::new();
        let mut patterns: BTreeSet<String> = BTreeSet::new();
        for value in &values {
            let v = value.expose_secret();
            for form in expand(v) {
                if form.len() >= MIN_VALUE_LEN {
                    patterns.insert(form);
                }
            }
            for part in derived_values(v) {
                if part.len() >= MIN_VALUE_LEN {
                    for form in expand(&part) {
                        if form.len() >= MIN_VALUE_LEN {
                            patterns.insert(form);
                        }
                    }
                }
            }
            distinct.insert(v);
        }
        let count = distinct.len();
        drop(distinct);
        if patterns.is_empty() {
            return Ok(KnownValues {
                matcher: None,
                count,
            });
        }
        let matcher = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&patterns)
            .map_err(|e| RedactError::Matcher(e.to_string()))?;
        Ok(KnownValues {
            matcher: Some(matcher),
            count,
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn find<'a>(&'a self, text: &[u8], out: &mut Vec<Span<'a>>) {
        if let Some(m) = &self.matcher {
            out.extend(m.find_iter(text).map(|m| Span {
                start: m.start(),
                end: m.end(),
                kind: KIND,
                priority: PRIORITY_KNOWN,
            }));
        }
    }
}

/// Secret-looking variables from an environment.
///
/// InLoop's own `INLOOP_*` variables (session IDs, paths) are skipped unless
/// the name says plainly that it holds a credential.
pub(crate) fn env_values(vars: impl Iterator<Item = (String, String)>) -> Vec<SecretString> {
    vars.filter(|(k, v)| !inloop_own_var(k) && name_looks_secret(k) && keep_value(k, v))
        .map(|(_, v)| SecretString::from(v))
        .collect()
}

fn inloop_own_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("INLOOP_")
        && !["SECRET", "TOKEN", "PASSWORD", "KEY"]
            .iter()
            .any(|w| upper.contains(w))
}

/// Files under `root` that match any of `globs`. A glob without `/` matches a
/// file name at any depth; one with `/` matches the path relative to `root`.
fn matching_files(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>, RedactError> {
    if globs.is_empty() {
        return Ok(Vec::new());
    }
    let (by_name, by_path) = build_globsets(globs)?;
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(source) => {
            return Err(RedactError::Io {
                path: root.to_path_buf(),
                source,
            });
        }
    };
    let mut stack: Vec<(PathBuf, usize)> = Vec::new();
    visit(entries, root, 0, &by_name, &by_path, &mut found, &mut stack);
    while let Some((dir, depth)) = stack.pop() {
        // Subdirectories we cannot list are skipped: we cannot know what is in them.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            visit(
                entries, root, depth, &by_name, &by_path, &mut found, &mut stack,
            );
        }
    }
    found.sort();
    Ok(found)
}

fn visit(
    entries: std::fs::ReadDir,
    root: &Path,
    depth: usize,
    by_name: &GlobSet,
    by_path: &GlobSet,
    found: &mut Vec<PathBuf>,
    stack: &mut Vec<(PathBuf, usize)>,
) {
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_dir() {
            if depth + 1 < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_ref()) {
                stack.push((path, depth + 1));
            }
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if by_name.is_match(name.as_ref()) || by_path.is_match(rel) {
            // A symlink to a directory is not a file of values.
            if std::fs::metadata(&path)
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            found.push(path);
        }
    }
}

pub(crate) fn build_globsets(globs: &[String]) -> Result<(GlobSet, GlobSet), RedactError> {
    let mut by_name = GlobSetBuilder::new();
    let mut by_path = GlobSetBuilder::new();
    for g in globs {
        let trimmed = g.trim_start_matches("./");
        let glob = GlobBuilder::new(trimmed)
            .literal_separator(true)
            .build()
            .map_err(|e| RedactError::BadGlob {
                glob: g.clone(),
                message: e.to_string(),
            })?;
        if trimmed.contains('/') {
            by_path.add(glob);
        } else {
            by_name.add(glob);
        }
    }
    let by_name = by_name.build().map_err(|e| RedactError::BadGlob {
        glob: globs.join(", "),
        message: e.to_string(),
    })?;
    let by_path = by_path.build().map_err(|e| RedactError::BadGlob {
        glob: globs.join(", "),
        message: e.to_string(),
    })?;
    Ok((by_name, by_path))
}

/// Parse `.env`-style text: `KEY=VALUE`, `export KEY=VALUE`, single and double
/// quotes (double-quoted values may span lines), `#` comments, blank lines.
pub(crate) fn parse_dotenv(text: &str) -> Vec<(String, SecretString)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        {
            continue;
        }
        let rest = rest.trim_start();
        let value = if let Some(body) = rest.strip_prefix('"') {
            let mut acc = String::new();
            let mut current = body.to_string();
            loop {
                if let Some(end) = closing_quote(&current) {
                    acc.push_str(&current[..end]);
                    break;
                }
                acc.push_str(&current);
                match lines.next() {
                    Some(next) => {
                        acc.push('\n');
                        current = next.to_string();
                    }
                    None => break,
                }
            }
            unescape_double(&acc)
        } else if let Some(body) = rest.strip_prefix('\'') {
            match body.find('\'') {
                Some(end) => body[..end].to_string(),
                None => body.to_string(),
            }
        } else {
            let v = match rest.find(" #") {
                Some(i) => &rest[..i],
                None => rest,
            };
            v.trim().to_string()
        };
        out.push((key.to_string(), SecretString::from(value)));
    }
    out
}

/// Index of the first double quote not escaped by a backslash.
fn closing_quote(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some(i),
            _ => escaped = false,
        }
    }
    None
}

fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Is this value worth redacting everywhere it appears?
pub(crate) fn keep_value(key: &str, value: &str) -> bool {
    let v = value.trim();
    if v.len() < MIN_VALUE_LEN {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "none" | "undefined" | "production" | "development"
    ) {
        return false;
    }
    // Pure numbers: ports, timeouts, IDs.
    if v.bytes()
        .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return false;
    }
    // References to other variables.
    if v.starts_with("${") || (v.starts_with('$') && !v.contains(' ') && v.len() < 64) {
        return false;
    }
    // File paths.
    if (v.starts_with('/') || v.starts_with("./") || v.starts_with("../") || v.starts_with("~/"))
        && !v.contains(char::is_whitespace)
        && !v.contains(':')
    {
        return false;
    }
    if url_has_password(v) {
        return true;
    }
    if let Some(after_scheme) = url_without_credentials(v) {
        // A URL with no user info is only secret if it carries a token-like
        // path or query segment (webhook URLs, signed URLs).
        return has_token_segment(after_scheme);
    }
    if name_looks_secret(key) {
        return true;
    }
    // A plain word, host name or version under a non-secret name is
    // configuration, not a secret.
    let has_digit = v.bytes().any(|b| b.is_ascii_digit());
    let has_alpha = v.bytes().any(|b| b.is_ascii_alphabetic());
    v.len() >= 12 && has_digit && has_alpha && crate::entropy::shannon(v.as_bytes()) >= 3.0
}

fn url_has_password(v: &str) -> bool {
    let Some((_, rest)) = v.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'))
}

/// For a URL without `user:pass@`, the part after `scheme://`.
fn url_without_credentials(v: &str) -> Option<&str> {
    let (scheme, rest) = v.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+')
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return None;
    }
    Some(rest)
}

fn has_token_segment(rest: &str) -> bool {
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("");
    if matches!(host_name, "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]") {
        return false;
    }
    rest[host.len()..]
        .split(['/', '?', '&', '=', '#'])
        .any(|seg| {
            seg.len() >= 16
                && seg
                    .bytes()
                    .any(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
                && crate::entropy::shannon(seg.as_bytes()) >= 3.0
        })
}

/// Secrets inside a value that may appear on their own: the password of a URL
/// with credentials, and the long lines of a multi-line value such as a PEM key.
fn derived_values(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((_, rest)) = v.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if let Some((userinfo, _)) = authority.rsplit_once('@')
            && let Some((_, password)) = userinfo.split_once(':')
        {
            out.push(password.to_string());
            if let Some(decoded) = percent_decode(password) {
                out.push(decoded);
            }
        }
    }
    if v.contains('\n') {
        for line in v.lines() {
            let line = line.trim();
            if line.len() >= 20 && !line.starts_with("-----") {
                out.push(line.to_string());
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Every form of `v` to match: the value itself, base64 (standard and
/// URL-safe, padded and not, and the fragments that appear when the value is
/// part of a longer base64 string), URL-encoded and JSON-escaped.
pub(crate) fn expand(v: &str) -> Vec<String> {
    let mut forms = vec![v.to_string()];
    let bytes = v.as_bytes();
    for engine in [&STANDARD, &URL_SAFE] {
        let padded = engine.encode(bytes);
        forms.push(padded.trim_end_matches('=').to_string());
        forms.push(padded);
        for shift in 1..3usize {
            if let Some(fragment) = base64_fragment(engine, bytes, shift) {
                forms.push(fragment);
            }
        }
        if let Some(fragment) = base64_fragment(engine, bytes, 0) {
            forms.push(fragment);
        }
    }
    forms.push(percent_encode(v, false));
    forms.push(percent_encode(v, true));
    forms.push(percent_encode(v, false).replace("%20", "+"));
    if let Ok(json) = serde_json::to_string(v) {
        let inner = json[1..json.len() - 1].to_string();
        forms.push(inner.replace('/', "\\/"));
        forms.push(inner);
    }
    let mut seen = BTreeSet::new();
    forms.retain(|f| seen.insert(f.clone()));
    forms
}

/// The base64 characters that depend only on `v` when `v` starts `shift`
/// bytes into a longer encoded string.
fn base64_fragment(
    engine: &base64::engine::GeneralPurpose,
    v: &[u8],
    shift: usize,
) -> Option<String> {
    let mut input = vec![0u8; shift];
    input.extend_from_slice(v);
    let encoded = engine.encode(&input);
    let first_bit = shift * 8;
    let last_bit = (shift + v.len()) * 8;
    let first_char = first_bit.div_ceil(6);
    let end_char = last_bit / 6;
    if end_char <= first_char || end_char - first_char < MIN_FRAGMENT_LEN {
        return None;
    }
    encoded.get(first_char..end_char).map(str::to_string)
}

fn percent_encode(v: &str, lower: bool) -> String {
    let mut out = String::with_capacity(v.len() * 3);
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else if lower {
            out.push_str(&format!("%{b:02x}"));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<(String, String)> {
        parse_dotenv(text)
            .into_iter()
            .map(|(k, v)| (k, v.expose_secret().to_string()))
            .collect()
    }

    #[test]
    fn parses_dotenv_forms() {
        let text = "# comment\n\nexport A=one-value # trailing\nB='single # quoted'\nC=\"double \\\"q\\\" \\n\"\nD=\"multi\nline\"\n  E = spaced \nnot a pair\n";
        assert_eq!(
            values(text),
            vec![
                ("A".into(), "one-value".into()),
                ("B".into(), "single # quoted".into()),
                ("C".into(), "double \"q\" \n".into()),
                ("D".into(), "multi\nline".into()),
                ("E".into(), "spaced".into()),
            ]
        );
    }

    #[test]
    fn filters_values_that_are_not_secret() {
        assert!(!keep_value("PORT", "30000000"));
        assert!(!keep_value("DEBUG", "true"));
        assert!(!keep_value("NODE_ENV", "production"));
        assert!(!keep_value("API_KEY", "short"));
        assert!(!keep_value("API_URL", "http://localhost:3000/api"));
        assert!(!keep_value("API_URL", "https://api.example.com/v1/items"));
        assert!(!keep_value("CERT_PATH", "/etc/ssl/certs/server.pem"));
        assert!(!keep_value("OTHER_SECRET", "${SOME_SECRET}"));
        assert!(!keep_value("APP_NAME", "MyCoolApplication"));
        assert!(keep_value("API_KEY", "plainlowercasebutsecret"));
        assert!(keep_value(
            "DATABASE_URL",
            "postgres://app:pw0rdpw0rd@db:5432/app"
        ));
        assert!(keep_value(
            "SLACK_HOOK",
            "https://hooks.slack.com/services/T0000/B0000/EXAMPLEfakeHook0123456"
        ));
        assert!(keep_value("STRIPE_SK", "rk_test_EXAMPLE0000fake11112222"));
    }

    #[test]
    fn base64_fragments_cover_misaligned_values() {
        let value = "correct-horse-battery";
        let forms = expand(value);
        for prefix in ["", "u", "us", "user:"] {
            let whole = STANDARD.encode(format!("{prefix}{value}trailing"));
            assert!(
                forms
                    .iter()
                    .any(|f| f.len() >= MIN_FRAGMENT_LEN && whole.contains(f.as_str())),
                "no fragment found for prefix {prefix:?}"
            );
        }
    }

    #[test]
    fn derived_values_include_url_passwords() {
        let d = derived_values("postgres://app:p%40ssw0rd-long@db.internal:5432/app");
        assert!(d.contains(&"p%40ssw0rd-long".to_string()));
        assert!(d.contains(&"p@ssw0rd-long".to_string()));
    }

    #[test]
    fn env_values_only_take_secret_names() {
        let vars = vec![
            (
                "PATH".to_string(),
                "/usr/bin:/bin:/opt/homebrew/bin".to_string(),
            ),
            ("HOME".to_string(), "/Users/example".to_string()),
            (
                "MY_SERVICE_TOKEN".to_string(),
                "tok-EXAMPLE-0000-fake".to_string(),
            ),
            (
                "SSH_AUTH_SOCK".to_string(),
                "/private/tmp/com.apple.launchd.x/Listeners".to_string(),
            ),
        ];
        let got: Vec<String> = env_values(vars.into_iter())
            .iter()
            .map(|v| v.expose_secret().to_string())
            .collect();
        assert_eq!(got, vec!["tok-EXAMPLE-0000-fake".to_string()]);
    }

    #[test]
    fn session_ids_and_inloop_vars_are_not_known_values() {
        let vars = vec![
            (
                "INLOOP_SESSION_ID".to_string(),
                "sess-1234-EXAMPLE-abcd".to_string(),
            ),
            (
                "INLOOP_AUTH_HANDLE".to_string(),
                "handle-EXAMPLE-0000-fake".to_string(),
            ),
            (
                "CLAUDE_SESSION_ID".to_string(),
                "5f0c2a8e-3b1d-4e6f-9a7c-2d4b8e1f0a63".to_string(),
            ),
            (
                "TERM_SESSION_ID".to_string(),
                "w0t0p0:2B7C1D3E-0000-4000-8000-EXAMPLE00001".to_string(),
            ),
            (
                "ITERM_SESSION_ID".to_string(),
                "w0t0p0:2B7C1D3E-0000-4000-8000-EXAMPLE00002".to_string(),
            ),
            ("SECURITYSESSIONID".to_string(), "186a5".to_string()),
            (
                "INLOOP_SYNC_TOKEN".to_string(),
                "tok-INLOOP-EXAMPLE-0000".to_string(),
            ),
        ];
        let got: Vec<String> = env_values(vars.into_iter())
            .iter()
            .map(|v| v.expose_secret().to_string())
            .collect();
        assert_eq!(got, vec!["tok-INLOOP-EXAMPLE-0000".to_string()]);
    }
}
