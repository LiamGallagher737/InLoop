//! Layer 3: context plus entropy.
//!
//! A value is redacted when its context says it is a credential and it looks
//! like one: `password=...`, `"api_key": "..."`, `client_secret: ...`,
//! `--token ...`, `Authorization: Bearer ...`, `Basic <base64 user:pass>`.
//! Values that look like code (`process.env.X`, `getToken()`), variable
//! references (`$TOKEN`, `${{ secrets.X }}`), placeholders and plain words are
//! left alone. Passwords embedded in URLs (`scheme://user:pass@host`) are
//! redacted too.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use regex::bytes::{Regex, RegexBuilder};

use crate::entropy::shannon;
use crate::{PRIORITY_CONTEXT, RedactError, Span};

pub(crate) const KIND: &str = "secret-in-context";
pub(crate) const URL_KIND: &str = "url-credentials";

/// The longest value considered.
const MAX_VALUE: usize = 512;

pub(crate) struct ContextLayer {
    keyword: Regex,
    url: Regex,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// password, passwd, pwd, passphrase: any non-trivial value.
    Password,
    /// secret, token, api key and similar: needs to look random.
    Token,
    /// `Authorization:` header: skip the scheme word, then a token.
    Authorization,
    /// `Bearer <token>` anywhere.
    Bearer,
    /// `Basic <base64>` anywhere.
    Basic,
}

impl ContextLayer {
    pub(crate) fn new() -> Result<ContextLayer, RedactError> {
        let build = |p: &str| {
            RegexBuilder::new(p)
                .unicode(false)
                .case_insensitive(true)
                .build()
                .map_err(|e| RedactError::Rules(e.to_string()))
        };
        let keyword = build(
            r"authorization|bearer|basic|passw(?:or)?d|passphrase|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|credentials?|(?:signing|encryption|master|session)[_-]?key",
        )?;
        let url = build(
            r#"\b[a-z][a-z0-9+.-]{1,30}://([^\s:/?#@"'<>\[\]{}|\\^`]{1,256}):([^\s/?#@"'<>\[\]{}|\\^`]{1,256})@[a-z0-9\[]"#,
        )?;
        Ok(ContextLayer { keyword, url })
    }

    pub(crate) fn find<'a>(&'a self, text: &[u8], out: &mut Vec<Span<'a>>) {
        for m in self.keyword.find_iter(text) {
            if let Some((start, end)) = self.value_after(text, m.start(), m.end()) {
                out.push(Span {
                    start,
                    end,
                    kind: KIND,
                    priority: PRIORITY_CONTEXT,
                });
            }
        }
        for caps in self.url.captures_iter(text) {
            let Some(pw) = caps.get(2) else { continue };
            if is_placeholder(pw.as_bytes()) {
                continue;
            }
            out.push(Span {
                start: pw.start(),
                end: pw.end(),
                kind: URL_KIND,
                priority: PRIORITY_CONTEXT,
            });
        }
    }

    /// The span of the credential that the keyword at `ks..ke` introduces, if any.
    fn value_after(&self, text: &[u8], ks: usize, ke: usize) -> Option<(usize, usize)> {
        let keyword = text[ks..ke].to_ascii_lowercase();
        // Widen to the whole identifier: `DB_PASSWORD`, `x-api-key`, `--token`.
        let mut ns = ks;
        while ns > 0 && is_name_byte(text[ns - 1]) {
            ns -= 1;
        }
        let mut ne = ke;
        while ne < text.len() && is_name_byte(text[ne]) {
            ne += 1;
        }
        let name = text[ns..ne].to_ascii_lowercase();
        let flag = name.starts_with(b"--");
        let class = match keyword.as_slice() {
            b"authorization" => Class::Authorization,
            b"bearer" if name == b"bearer" => Class::Bearer,
            b"basic" if name == b"basic" => Class::Basic,
            b"bearer" | b"basic" => return None,
            b"password" | b"passwd" | b"passphrase" | b"pwd" => Class::Password,
            _ => Class::Token,
        };
        if matches!(class, Class::Password | Class::Token) && name_is_not_secret(&name) {
            return None;
        }

        let mut i = ne;
        match class {
            Class::Bearer | Class::Basic => {
                i = skip_spaces(text, i);
                if i == ne {
                    return None;
                }
            }
            _ => {
                // Closing quote of a quoted key (escaped inside JSON strings),
                // then the separator.
                let j = skip_backslashes(text, i);
                if j < text.len() && matches!(text[j], b'"' | b'\'' | b'`') {
                    i = j + 1;
                }
                let before_sep = i;
                i = skip_spaces(text, i);
                const SEPARATORS: [&[u8]; 4] = [b"=>", b":=", b"=", b":"];
                let sep = SEPARATORS.iter().find(|s| text[i..].starts_with(s));
                match sep {
                    Some(s) => {
                        i += s.len();
                        if *s == b":" {
                            i = skip_type_annotation(text, i);
                        }
                    }
                    // `--password value` on a command line.
                    None if flag && i > before_sep => {}
                    None => return None,
                }
                i = skip_spaces(text, i);
            }
        }
        let j = skip_backslashes(text, i);
        let quote = match text.get(j) {
            Some(&q @ (b'"' | b'\'' | b'`')) => {
                i = j + 1;
                Some(q)
            }
            _ => None,
        };
        let mut class = class;
        if class == Class::Authorization {
            // Skip the scheme: `Bearer`, `Basic`, `token`, `Digest`...
            let word_end = i + text[i..]
                .iter()
                .take_while(|b| b.is_ascii_alphabetic() || **b == b'-')
                .count();
            let after = skip_spaces(text, word_end);
            if word_end > i && after > word_end && after < text.len() {
                if text[i..word_end].eq_ignore_ascii_case(b"basic") {
                    class = Class::Basic;
                }
                i = after;
            }
        }
        let vs = i;
        let mut ve = vs;
        match quote {
            Some(q) => {
                while ve < text.len()
                    && ve - vs < MAX_VALUE
                    && text[ve] != q
                    && text[ve] != b'\n'
                    && !(text[ve] == b'\\' && text.get(ve + 1) == Some(&q))
                {
                    ve += 1;
                }
            }
            None => {
                while ve < text.len() && ve - vs < MAX_VALUE && is_value_byte(text[ve]) {
                    ve += 1;
                }
            }
        }
        // Trailing sentence punctuation is not part of the value.
        while ve > vs && matches!(text[ve - 1], b'.' | b',' | b';' | b')') {
            ve -= 1;
        }
        let value = &text[vs..ve];
        if value.is_empty() || is_placeholder(value) {
            return None;
        }
        // A function call, not a value.
        if quote.is_none() && text.get(ve) == Some(&b'(') {
            return None;
        }
        let ok = match class {
            Class::Password => password_like(value, quote.is_some()),
            Class::Token | Class::Authorization => token_like(value, 16),
            Class::Bearer => token_like(value, 12),
            Class::Basic => basic_credentials(value) || token_like(value, 16),
        };
        ok.then_some((vs, ve))
    }
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'+' | b'/'
                | b'='
                | b'_'
                | b'-'
                | b'.'
                | b'~'
                | b'!'
                | b'@'
                | b'$'
                | b'%'
                | b'^'
                | b'*'
        )
}

/// `const SECRET: &str = "..."`, `api_key: str = "..."`: when a `:` is
/// followed by a type and then `=`, the value comes after the `=`.
fn skip_type_annotation(text: &[u8], start: usize) -> usize {
    let mut i = start;
    let limit = (start + 60).min(text.len());
    let mut saw_type = false;
    while i < limit {
        let b = text[i];
        if b == b'=' {
            let next = text.get(i + 1).copied();
            if saw_type && !matches!(next, Some(b'=') | Some(b'>')) {
                return i + 1;
            }
            return start;
        }
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'&') {
            saw_type = true;
        } else if !matches!(
            b,
            b' ' | b'\t' | b'<' | b'>' | b',' | b'\'' | b'[' | b']' | b':' | b'?'
        ) {
            return start;
        }
        i += 1;
    }
    start
}

fn skip_backslashes(text: &[u8], mut i: usize) -> usize {
    while i < text.len() && text[i] == b'\\' {
        i += 1;
    }
    i
}

fn skip_spaces(text: &[u8], mut i: usize) -> usize {
    while i < text.len() && matches!(text[i], b' ' | b'\t') {
        i += 1;
    }
    i
}

/// Field names that contain a keyword but hold something else.
fn name_is_not_secret(name: &[u8]) -> bool {
    const PARTS: &[&str] = &[
        "endpoint",
        "url",
        "uri",
        "file",
        "path",
        "name",
        "length",
        "size",
        "type",
        "count",
        "expir",
        "ttl",
        "header",
        "field",
        "prefix",
        "author",
        "tokeniz",
        "tokens",
        "usage",
        "limit",
        "budget",
        "max",
        "min",
        "version",
        "format",
        "scope",
        "pattern",
        "regex",
        "label",
        "hint",
        "policy",
        "rotation",
        "timeout",
        "sock",
        "dir",
        "mode",
        "method",
        "algorithm",
        "enabled",
        "required",
        "valid",
        "manager",
        "provider",
        "store",
        "_id",
        "-id",
        "ref",
        "placeholder",
        "template",
        "index",
        "keychain",
        "secretsid",
        "csrf",
        "public",
        "reset",
        "confirm",
        "strength",
        "policy",
        "changed",
        "_at",
    ];
    let name = String::from_utf8_lossy(name);
    PARTS.iter().any(|p| name.contains(p))
}

fn is_placeholder(v: &[u8]) -> bool {
    let first = v[0];
    if matches!(first, b'$' | b'<' | b'{' | b'%' | b'[' | b'(') {
        return true;
    }
    if v.iter()
        .all(|&b| matches!(b, b'*' | b'x' | b'X' | b'.' | b'-' | b'#'))
    {
        return true;
    }
    let lower = v.to_ascii_lowercase();
    matches!(
        lower.as_slice(),
        b"true"
            | b"false"
            | b"null"
            | b"none"
            | b"nil"
            | b"undefined"
            | b"redacted"
            | b"changeme"
            | b"password"
            | b"secret"
            | b"string"
            | b"optional"
    ) || lower.starts_with(b"your")
        || lower.starts_with(b"redacted")
}

/// `process.env.X`, `self.password`, `getPassword`, `DB_PASSWORD`, `required`:
/// shapes that are code, not secrets.
fn looks_like_code(v: &[u8]) -> bool {
    let trimmed: &[u8] = {
        let mut end = v.len();
        while end > 0 && matches!(v[end - 1], b'!' | b'?' | b';') {
            end -= 1;
        }
        &v[..end]
    };
    if trimmed.is_empty() {
        return true;
    }
    let ident = |s: &[u8]| {
        !s.is_empty()
            && (s[0].is_ascii_alphabetic() || s[0] == b'_' || s[0] == b'$')
            && s.iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
    };
    let parts: Vec<&[u8]> = trimmed.split(|&b| b == b'.').collect();
    if parts.len() > 1 && parts.iter().all(|p| ident(p)) {
        // Dotted path, unless it looks like a random token with dots.
        return !parts
            .iter()
            .any(|p| p.iter().any(u8::is_ascii_digit) && p.len() > 12);
    }
    if !ident(trimmed) {
        return false;
    }
    let has_digit = trimmed.iter().any(u8::is_ascii_digit);
    let has_lower = trimmed.iter().any(u8::is_ascii_lowercase);
    let has_upper = trimmed.iter().any(u8::is_ascii_uppercase);
    // CONSTANT_CASE, lower_snake, or camelCase words without digits.
    !has_digit && (!(has_lower && has_upper) || camel_words(trimmed))
}

/// `getPassword`, `apiKeyValue`: lower start, then capitalised words.
fn camel_words(s: &[u8]) -> bool {
    if !s[0].is_ascii_lowercase() {
        return false;
    }
    // Every uppercase letter is followed by a lowercase one (or ends the word).
    s.windows(2)
        .all(|w| !w[0].is_ascii_uppercase() || w[1].is_ascii_lowercase())
}

fn password_like(v: &[u8], quoted: bool) -> bool {
    if v.len() < 6 || v.contains(&b' ') && !quoted {
        return false;
    }
    if v.starts_with(b"/") || v.starts_with(b"./") || v.starts_with(b"~/") {
        return false;
    }
    if looks_like_code(v) {
        return false;
    }
    // Plain words and short phrases (form labels, prose).
    !v.iter().all(|b| b.is_ascii_alphabetic() || *b == b' ')
}

fn token_like(v: &[u8], min_len: usize) -> bool {
    if v.len() < min_len || v.contains(&b' ') {
        return false;
    }
    if looks_like_code(v) {
        return false;
    }
    let has_digit = v.iter().any(u8::is_ascii_digit);
    let has_alpha = v.iter().any(u8::is_ascii_alphabetic);
    if !has_alpha || (!has_digit && v.len() < 24) {
        return false;
    }
    shannon(v) >= 3.0
}

/// `Basic dXNlcjpwYXNz`: base64 of printable `user:password`.
fn basic_credentials(v: &[u8]) -> bool {
    if v.len() < 8 {
        return false;
    }
    let Ok(decoded) = STANDARD.decode(v) else {
        return false;
    };
    decoded.contains(&b':')
        && decoded.len() >= 3
        && decoded.iter().all(|b| (0x20..0x7f).contains(b))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn redacted(text: &str) -> Vec<String> {
        let layer = ContextLayer::new().unwrap();
        let mut spans = Vec::new();
        layer.find(text.as_bytes(), &mut spans);
        spans
            .iter()
            .map(|s| text[s.start..s.end].to_string())
            .collect()
    }

    #[test]
    fn catches_credentials_in_context() {
        let cases = [
            ("DB_PASSWORD=Tr0ub4dor&3x", "Tr0ub4dor"),
            ("password: \"correct horse 42\"", "correct horse 42"),
            (
                "\"client_secret\": \"Zm9vYmFyEXAMPLE0fake9q8w7e6r\"",
                "Zm9vYmFyEXAMPLE0fake9q8w7e6r",
            ),
            (
                "api_key = 'a1b2c3d4e5f6EXAMPLE7g8h9'",
                "a1b2c3d4e5f6EXAMPLE7g8h9",
            ),
            (
                "Authorization: Bearer abc123EXAMPLEfake456def",
                "abc123EXAMPLEfake456def",
            ),
            (
                "Authorization: token 0fake1EXAMPLE2token3value",
                "0fake1EXAMPLE2token3value",
            ),
            (
                "curl -H 'Authorization: Basic dXNlcjpFWEFNUExFLXBhc3M='",
                "dXNlcjpFWEFNUExFLXBhc3M=",
            ),
            (
                "mytool --token f4k3T0k3nEXAMPLE0000abcd",
                "f4k3T0k3nEXAMPLE0000abcd",
            ),
            (
                "x-api-key: k3y-EXAMPLE-0000-fake-1234",
                "k3y-EXAMPLE-0000-fake-1234",
            ),
            (
                "const CLIENT_SECRET: &str = \"cs_EXAMPLE_0f4k3c1i3nt9q8w\";",
                "cs_EXAMPLE_0f4k3c1i3nt9q8w",
            ),
            (
                "api_key: Optional[str] = 'k3yEXAMPLE0000fake1234'",
                "k3yEXAMPLE0000fake1234",
            ),
            (
                "postgres://admin:hunter2EXAMPLE@db.example.com:5432/app",
                "hunter2EXAMPLE",
            ),
        ];
        for (text, want) in cases {
            let got = redacted(text);
            assert!(got.iter().any(|g| g == want), "{text}: got {got:?}");
        }
    }

    #[test]
    fn leaves_code_and_placeholders_alone() {
        for text in [
            "password: process.env.DB_PASSWORD",
            "const token = getToken();",
            "api_key: str = Field(default=None)",
            "secret: ${{ secrets.DEPLOY_KEY }}",
            "password=$DB_PASSWORD",
            "token_count: 12345",
            "\"input_tokens\": 4096",
            "password: string;",
            "Bearer authentication is required",
            "Basic usage of the API",
            "the token is valid",
            "access_key_id = AKIA_LOOKS_LIKE_NAME",
            "password_reset_url: https://example.com/reset",
            "secretName: my-app-secret",
            "token=***",
            "password: <your-password>",
            "postgres://user:${DB_PASS}@localhost/app",
            "\"passwordField\": \"Password\"",
            "tokenizer: cl100k_base",
            "cache_key = sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        ] {
            assert!(redacted(text).is_empty(), "{text}: {:?}", redacted(text));
        }
    }
}
