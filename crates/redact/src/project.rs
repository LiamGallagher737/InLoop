//! Layer 5: project rules.
//!
//! Extra patterns from `inloop.toml`, and, when `personal_data` is on,
//! built-in rules for email addresses, phone numbers and card numbers. IP
//! addresses are deliberately not redacted: they are rarely personal in a
//! development transcript and redacting them ruins logs.

use regex::{Regex, RegexBuilder};

use crate::{PRIORITY_PROJECT, PatternRule, RedactError, Span};

struct Rule {
    kind: String,
    regex: Regex,
    secret_group: Option<usize>,
}

enum Check {
    Email,
    Phone,
    Card,
}

struct PersonalRule {
    kind: &'static str,
    regex: Regex,
    check: Check,
}

pub(crate) struct ProjectRules {
    rules: Vec<Rule>,
    personal: Vec<PersonalRule>,
}

fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= 64
        && kind
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

impl ProjectRules {
    pub(crate) fn new(patterns: &[PatternRule], personal_data: bool) -> Result<Self, RedactError> {
        let mut rules = Vec::with_capacity(patterns.len());
        for p in patterns {
            if !valid_kind(&p.kind) {
                return Err(RedactError::BadPattern {
                    kind: p.kind.clone(),
                    message: "kind must be 1 to 64 ASCII letters, digits, '_', '.' or '-'"
                        .to_string(),
                });
            }
            let regex = RegexBuilder::new(&p.regex)
                .size_limit(16 << 20)
                .build()
                .map_err(|e| RedactError::BadPattern {
                    kind: p.kind.clone(),
                    message: e.to_string(),
                })?;
            let secret_group = regex.capture_names().position(|n| n == Some("secret"));
            rules.push(Rule {
                kind: p.kind.clone(),
                regex,
                secret_group,
            });
        }
        let personal = if personal_data {
            personal_rules()?
        } else {
            Vec::new()
        };
        Ok(ProjectRules { rules, personal })
    }

    pub(crate) fn count(&self) -> usize {
        self.rules.len() + self.personal.len()
    }

    pub(crate) fn find<'a>(&'a self, text: &str, out: &mut Vec<Span<'a>>) {
        for rule in &self.rules {
            match rule.secret_group {
                Some(g) => {
                    for caps in rule.regex.captures_iter(text) {
                        if let Some(m) = caps.get(g).filter(|m| !m.is_empty()) {
                            out.push(span(m.start(), m.end(), &rule.kind));
                        }
                    }
                }
                None => {
                    for m in rule.regex.find_iter(text).filter(|m| !m.is_empty()) {
                        out.push(span(m.start(), m.end(), &rule.kind));
                    }
                }
            }
        }
        for rule in &self.personal {
            for m in rule.regex.find_iter(text) {
                let ok = match rule.check {
                    Check::Email => email_ok(m.as_str()),
                    Check::Phone => phone_ok(text, m.start(), m.end()),
                    Check::Card => card_ok(text, m.start(), m.end()),
                };
                if ok {
                    out.push(span(m.start(), m.end(), rule.kind));
                }
            }
        }
    }
}

fn span(start: usize, end: usize, kind: &str) -> Span<'_> {
    Span {
        start,
        end,
        kind,
        priority: PRIORITY_PROJECT,
    }
}

fn personal_rules() -> Result<Vec<PersonalRule>, RedactError> {
    let build = |p: &str| {
        RegexBuilder::new(p)
            .unicode(false)
            .build()
            .map_err(|e| RedactError::Rules(e.to_string()))
    };
    Ok(vec![
        PersonalRule {
            kind: "email",
            regex: build(
                r"\b[A-Za-z0-9][A-Za-z0-9._%+-]{0,63}@[A-Za-z0-9](?:[A-Za-z0-9-]{0,62}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,62}[A-Za-z0-9])?)*\.[A-Za-z]{2,24}\b",
            )?,
            check: Check::Email,
        },
        PersonalRule {
            kind: "phone",
            regex: build(concat!(
                // International: +64 21 123 4567, +1 (415) 555-0132, +442079460000
                r"\+[1-9][0-9]{0,2}(?:[ .-]?\(?[0-9]{1,4}\)?){1,5}[ .-]?[0-9]{2,5}",
                // US and similar: (415) 555-0132, 415-555-0132, 415.555.0132
                r"|\([0-9]{3}\) ?[0-9]{3}[-. ][0-9]{4}|\b[0-9]{3}[-.][0-9]{3}[-.][0-9]{4}\b",
                // NZ, AU, UK with a trunk 0: 021 123 4567, (09) 123 4567,
                // 0412 345 678, 020 7946 0000, 07700 900123
                r"|\(0[0-9]{1,4}\) ?[0-9]{3,4}[ -]?[0-9]{3,4}",
                r"|\b0[0-9]{1,4}[ -][0-9]{3,4}[ -]?[0-9]{3,4}\b",
            ))?,
            check: Check::Phone,
        },
        PersonalRule {
            kind: "card-number",
            regex: build(r"\b[3-6][0-9]{3}(?:[ -]?[0-9]{2,4}){2,4}[ -]?[0-9]{1,4}\b")?,
            check: Check::Card,
        },
    ])
}

fn email_ok(m: &str) -> bool {
    let Some((local, domain)) = m.rsplit_once('@') else {
        return false;
    };
    let local = local.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    // Automated addresses, not people.
    if matches!(
        local.as_str(),
        "git" | "noreply" | "no-reply" | "donotreply" | "mailer-daemon"
    ) || domain.ends_with("users.noreply.github.com")
    {
        return false;
    }
    // `image@2x.png`, `logo@3x.webp`: asset names, not addresses.
    let tld = domain.rsplit('.').next().unwrap_or("");
    !matches!(
        tld,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "js" | "ts" | "css" | "json" | "rs"
    )
}

fn digit_count(s: &str) -> usize {
    s.bytes().filter(u8::is_ascii_digit).count()
}

/// Not part of a longer number, version, date, path or `line:col`.
fn standalone(text: &str, start: usize, end: usize) -> bool {
    let bytes = text.as_bytes();
    let before = start.checked_sub(1).map(|i| bytes[i]);
    let after = bytes.get(end).copied();
    let after2 = bytes.get(end + 1).copied();
    let bad_before = before.is_some_and(|b| {
        b.is_ascii_alphanumeric()
            || matches!(b, b'.' | b':' | b'/' | b'-' | b'_' | b'#' | b'=' | b'v')
    });
    let bad_after = after.is_some_and(|b| {
        b.is_ascii_alphanumeric()
            || matches!(b, b':' | b'/' | b'-' | b'_')
            || (matches!(b, b'.') && after2.is_some_and(|c| c.is_ascii_alphanumeric()))
    });
    !bad_before && !bad_after
}

fn phone_ok(text: &str, start: usize, end: usize) -> bool {
    let m = &text[start..end];
    let digits = digit_count(m);
    if !(8..=15).contains(&digits) {
        return false;
    }
    if !standalone(text, start, end) {
        return false;
    }
    // A run of digits with no separators is only a phone number with a `+`.
    let separators = m
        .bytes()
        .filter(|b| matches!(b, b' ' | b'-' | b'.' | b'(' | b')'))
        .count();
    if separators == 0 && !m.starts_with('+') {
        return false;
    }
    // Dates: 2026-11-14, 2026.11.14 and times.
    let parts: Vec<&str> = m.split([' ', '-', '.']).collect();
    let looks_like_date =
        parts.len() == 3 && parts[0].len() == 4 && parts[1].len() <= 2 && parts[2].len() <= 2;
    !looks_like_date
}

fn card_ok(text: &str, start: usize, end: usize) -> bool {
    let m = &text[start..end];
    let digits: Vec<u32> = m.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) || !standalone(text, start, end) {
        return false;
    }
    // Separators, if any, must be one kind.
    let has_space = m.contains(' ');
    let has_dash = m.contains('-');
    if has_space && has_dash {
        return false;
    }
    luhn(&digits)
}

fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, String)> {
        let rules = ProjectRules::new(&[], true).unwrap();
        let mut spans = Vec::new();
        rules.find(text, &mut spans);
        spans
            .iter()
            .map(|s| (s.kind.to_string(), text[s.start..s.end].to_string()))
            .collect()
    }

    #[test]
    fn luhn_check() {
        assert!(luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
        assert!(!luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2]));
    }

    #[test]
    fn personal_data_is_found() {
        for (text, kind, want) in [
            (
                "mail jane.doe@example.com now",
                "email",
                "jane.doe@example.com",
            ),
            ("call +64 21 555 0199.", "phone", "+64 21 555 0199"),
            ("call (415) 555-0132", "phone", "(415) 555-0132"),
            ("call 415-555-0132", "phone", "415-555-0132"),
            ("ring 021 555 0199", "phone", "021 555 0199"),
            ("ring (09) 555 0199", "phone", "(09) 555 0199"),
            ("ring 0412 555 019", "phone", "0412 555 019"),
            ("ring 020 7946 0000", "phone", "020 7946 0000"),
            (
                "card 4111 1111 1111 1111 ok",
                "card-number",
                "4111 1111 1111 1111",
            ),
            (
                "card 5555555555554444 ok",
                "card-number",
                "5555555555554444",
            ),
        ] {
            let got = found(text);
            assert!(
                got.iter().any(|(k, v)| k == kind && v == want),
                "{text}: {got:?}"
            );
        }
    }

    #[test]
    fn look_alikes_are_not_personal_data() {
        for text in [
            "released 2026-11-14 at 10:15",
            "version 1.2.3 and 10.0.19045.3693",
            "src/main.rs:120:15",
            "commit 4f2a9c1e8b7d6a5f4e3c2b1a0f9e8d7c6b5a4f3e",
            "git@github.com:example/repo.git",
            "Co-Authored-By: Claude <noreply@anthropic.com>",
            "logo@2x.png",
            "id 1234567890123456 fails luhn",
            "pid 48213 port 5432 took 1500 ms",
            "uuid 123e4567-e89b-12d3-a456-426614174000",
            "ip 192.168.1.20 and 10.0.0.1",
            "build 20261114.1203",
            "react@18.3.1 and @types/node@22.1.0",
        ] {
            let got = found(text);
            assert!(got.is_empty(), "{text}: {got:?}");
        }
    }

    #[test]
    fn bad_patterns_fail_closed() {
        let bad = [PatternRule {
            kind: "customer".to_string(),
            regex: "(unclosed".to_string(),
        }];
        assert!(matches!(
            ProjectRules::new(&bad, false),
            Err(RedactError::BadPattern { .. })
        ));
        let bad_kind = [PatternRule {
            kind: "has space]".to_string(),
            regex: "x".to_string(),
        }];
        assert!(ProjectRules::new(&bad_kind, false).is_err());
    }
}
