//! Layer 2: provider patterns.
//!
//! The gitleaks default ruleset (v8.30.1, MIT licence, see
//! THIRD_PARTY_NOTICES.md) is vendored as `rules/gitleaks.toml` and compiled at
//! start-up, alongside InLoop's own `rules/builtin.toml`. The semantics follow
//! gitleaks: keywords prefilter which rules run, the secret is `secretGroup` or
//! the first non-empty capture group, the per-rule entropy threshold applies,
//! and the global and per-rule allowlists (regexes and stopwords) are honoured.
//!
//! Regexes are compiled as byte regexes with Unicode off, which matches Go's
//! RE2 semantics for `\w`, `\s`, `\b` and case folding.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use regex::bytes::{Regex, RegexBuilder};
use serde::Deserialize;

use crate::entropy::shannon;
use crate::{PRIORITY_BUILTIN, PRIORITY_GENERIC, PRIORITY_GITLEAKS, RedactError, Span};

const GITLEAKS_RULES: &str = include_str!("../rules/gitleaks.toml");
const BUILTIN_RULES: &str = include_str!("../rules/builtin.toml");

/// Rules that gitleaks scopes to a file path. Without a path they are either
/// meaningless (a binary file type) or too noisy; layer 3 covers the cases
/// that matter. `kubernetes-secret-yaml` is kept: its regex is specific enough
/// to run on any text.
const PATH_SCOPED_SKIP: &[&str] = &[
    "pkcs12-file",
    "freemius-secret-key",
    "hashicorp-tf-password",
    "nuget-config-password",
];

#[derive(Deserialize)]
struct RuleFile {
    #[serde(default)]
    allowlist: Option<RawAllowlist>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRule {
    id: String,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    entropy: Option<f64>,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    secret_group: Option<usize>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    allowlists: Vec<RawAllowlist>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAllowlist {
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    commits: Vec<String>,
    #[serde(default)]
    regexes: Vec<String>,
    #[serde(default)]
    regex_target: Option<String>,
    #[serde(default)]
    stopwords: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Secret,
    Match,
    Line,
}

struct Allowlist {
    and: bool,
    /// Paths and commits can never match here: there is no file or commit.
    has_unmatchable: bool,
    target: Target,
    regexes: Vec<Regex>,
    stopwords: Option<AhoCorasick>,
}

impl Allowlist {
    fn compile(raw: &RawAllowlist) -> Result<Allowlist, RedactError> {
        let target = match raw.regex_target.as_deref() {
            Some("match") => Target::Match,
            Some("line") => Target::Line,
            _ => Target::Secret,
        };
        // An allowlist regex that does not compile is dropped: that can only
        // make redaction stricter.
        let regexes = raw.regexes.iter().filter_map(|r| compile(r).ok()).collect();
        let stopwords = if raw.stopwords.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .build(&raw.stopwords)
                    .map_err(|e| RedactError::Rules(e.to_string()))?,
            )
        };
        Ok(Allowlist {
            and: raw.condition.as_deref() == Some("AND"),
            has_unmatchable: !raw.paths.is_empty() || !raw.commits.is_empty(),
            target,
            regexes,
            stopwords,
        })
    }

    fn allows(&self, secret: &[u8], whole: &[u8], line: &[u8]) -> bool {
        let target = match self.target {
            Target::Secret => secret,
            Target::Match => whole,
            Target::Line => line,
        };
        let regex_hit = self.regexes.iter().any(|r| r.is_match(target));
        let stop_hit = self.stopwords.as_ref().is_some_and(|s| s.is_match(secret));
        if self.and {
            if self.has_unmatchable || (self.regexes.is_empty() && self.stopwords.is_none()) {
                return false;
            }
            (self.regexes.is_empty() || regex_hit) && (self.stopwords.is_none() || stop_hit)
        } else {
            regex_hit || stop_hit
        }
    }
}

struct Rule {
    id: String,
    regex: Regex,
    entropy: Option<f64>,
    secret_group: Option<usize>,
    allowlists: Vec<Allowlist>,
    generic: bool,
    priority: u8,
}

pub(crate) struct ProviderRules {
    rules: Vec<Rule>,
    /// Keyword prefilter: pattern index to the rules that keyword unlocks.
    keywords: Option<AhoCorasick>,
    keyword_rules: Vec<Vec<usize>>,
    /// Rules with no keywords run on every input.
    always: Vec<usize>,
    global: Option<Allowlist>,
    gitleaks_compiled: usize,
    builtin_compiled: usize,
    skipped: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    skipped_ids: Vec<String>,
}

fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .unicode(false)
        .size_limit(64 << 20)
        .dfa_size_limit(64 << 20)
        .build()
}

impl ProviderRules {
    pub(crate) fn load() -> Result<ProviderRules, RedactError> {
        let gitleaks: RuleFile =
            toml::from_str(GITLEAKS_RULES).map_err(|e| RedactError::Rules(e.to_string()))?;
        let builtin: RuleFile =
            toml::from_str(BUILTIN_RULES).map_err(|e| RedactError::Rules(e.to_string()))?;

        let global = gitleaks
            .allowlist
            .as_ref()
            .map(Allowlist::compile)
            .transpose()?;
        let mut rules = Vec::new();
        let mut keyword_list: Vec<String> = Vec::new();
        let mut keyword_rules: Vec<Vec<usize>> = Vec::new();
        let mut always = Vec::new();
        let mut skipped = 0;
        let mut skipped_ids = Vec::new();
        let mut gitleaks_compiled = 0;
        let mut builtin_compiled = 0;

        for (raw, is_builtin) in gitleaks
            .rules
            .iter()
            .map(|r| (r, false))
            .chain(builtin.rules.iter().map(|r| (r, true)))
        {
            let Some(pattern) = raw.regex.as_deref() else {
                skipped += 1;
                skipped_ids.push(raw.id.clone());
                continue;
            };
            if !is_builtin && raw.path.is_some() && PATH_SCOPED_SKIP.contains(&raw.id.as_str()) {
                skipped += 1;
                skipped_ids.push(raw.id.clone());
                continue;
            }
            let regex = match compile(pattern) {
                Ok(r) => r,
                Err(e) if is_builtin => {
                    return Err(RedactError::Rules(format!("built-in rule {}: {e}", raw.id)));
                }
                Err(_) => {
                    skipped += 1;
                    skipped_ids.push(raw.id.clone());
                    continue;
                }
            };
            let allowlists = raw
                .allowlists
                .iter()
                .map(Allowlist::compile)
                .collect::<Result<Vec<_>, _>>()?;
            let generic = raw.id.starts_with("generic") || raw.id.starts_with("curl-");
            let priority = if is_builtin {
                PRIORITY_BUILTIN
            } else if generic {
                PRIORITY_GENERIC
            } else {
                PRIORITY_GITLEAKS
            };
            let index = rules.len();
            rules.push(Rule {
                id: raw.id.clone(),
                regex,
                entropy: if is_builtin { None } else { raw.entropy },
                secret_group: raw.secret_group,
                allowlists,
                generic,
                priority,
            });
            if is_builtin {
                builtin_compiled += 1;
            } else {
                gitleaks_compiled += 1;
            }
            if raw.keywords.is_empty() {
                always.push(index);
            }
            for k in &raw.keywords {
                let k = k.to_ascii_lowercase();
                match keyword_list.iter().position(|x| *x == k) {
                    Some(p) => keyword_rules[p].push(index),
                    None => {
                        keyword_list.push(k);
                        keyword_rules.push(vec![index]);
                    }
                }
            }
        }

        let keywords = if keyword_list.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .match_kind(MatchKind::Standard)
                    .build(&keyword_list)
                    .map_err(|e| RedactError::Rules(e.to_string()))?,
            )
        };

        Ok(ProviderRules {
            rules,
            keywords,
            keyword_rules,
            always,
            global,
            gitleaks_compiled,
            builtin_compiled,
            skipped,
            skipped_ids,
        })
    }

    pub(crate) fn compiled(&self) -> usize {
        self.gitleaks_compiled + self.builtin_compiled
    }

    pub(crate) fn skipped(&self) -> usize {
        self.skipped
    }

    #[cfg(test)]
    pub(crate) fn skipped_ids(&self) -> &[String] {
        &self.skipped_ids
    }

    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.gitleaks_compiled, self.builtin_compiled)
    }

    /// Indices of the rules whose keywords occur in `text`.
    fn triggered(&self, text: &[u8]) -> Vec<usize> {
        let mut on = vec![false; self.rules.len()];
        for &i in &self.always {
            on[i] = true;
        }
        if let Some(ac) = &self.keywords {
            let mut seen = vec![false; self.keyword_rules.len()];
            let mut remaining = seen.len();
            for m in ac.find_overlapping_iter(text) {
                let p = m.pattern().as_usize();
                if !seen[p] {
                    seen[p] = true;
                    for &r in &self.keyword_rules[p] {
                        on[r] = true;
                    }
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
            }
        }
        on.iter()
            .enumerate()
            .filter_map(|(i, &b)| b.then_some(i))
            .collect()
    }

    pub(crate) fn find<'a>(&'a self, text: &[u8], out: &mut Vec<Span<'a>>) {
        for index in self.triggered(text) {
            let rule = &self.rules[index];
            for caps in rule.regex.captures_iter(text) {
                let Some(whole) = caps.get(0) else {
                    continue;
                };
                let secret = match rule.secret_group {
                    Some(g) => caps.get(g),
                    None => caps
                        .iter()
                        .skip(1)
                        .flatten()
                        .find(|m| !m.is_empty())
                        .or(Some(whole)),
                };
                let Some(secret) = secret else {
                    continue;
                };
                if secret.is_empty() {
                    continue;
                }
                let bytes = secret.as_bytes();
                // gitleaks' lazy private-key regex runs on past a key with no
                // END line into the next PEM block; the built-in rule covers
                // truncated keys without swallowing certificates.
                if rule.priority != PRIORITY_BUILTIN
                    && rule.id == "private-key"
                    && bytes.windows(10).filter(|w| *w == b"-----BEGIN").count() > 1
                {
                    continue;
                }
                if let Some(threshold) = rule.entropy {
                    if shannon(bytes) <= threshold {
                        continue;
                    }
                    if rule.generic && !bytes.iter().any(u8::is_ascii_digit) {
                        continue;
                    }
                }
                let line = line_around(text, whole.start(), whole.end());
                if self
                    .global
                    .as_ref()
                    .is_some_and(|a| a.allows(bytes, whole.as_bytes(), line))
                {
                    continue;
                }
                if rule
                    .allowlists
                    .iter()
                    .any(|a| a.allows(bytes, whole.as_bytes(), line))
                {
                    continue;
                }
                out.push(Span {
                    start: secret.start(),
                    end: secret.end(),
                    kind: &rule.id,
                    priority: rule.priority,
                });
            }
        }
    }
}

fn line_around(text: &[u8], start: usize, end: usize) -> &[u8] {
    let line_start = text[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let line_end = text[end..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |i| end + i);
    &text[line_start..line_end]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The number of gitleaks rules we cannot use. If a ruleset update pushes
    /// this up, look at which rules broke before raising the threshold.
    const MAX_SKIPPED: usize = 4;

    #[test]
    fn gitleaks_rules_compile() {
        let rules = ProviderRules::load().unwrap();
        let (gitleaks, builtin) = rules.counts();
        eprintln!(
            "gitleaks compiled {gitleaks}, built-in {builtin}, skipped {}: {:?}",
            rules.skipped(),
            rules.skipped_ids()
        );
        assert!(gitleaks >= 210, "only {gitleaks} gitleaks rules compiled");
        assert!(
            rules.skipped() <= MAX_SKIPPED,
            "{} skipped: {:?}",
            rules.skipped(),
            rules.skipped_ids()
        );
    }

    fn kinds(text: &str) -> Vec<String> {
        let rules = ProviderRules::load().unwrap();
        let mut spans = Vec::new();
        rules.find(text.as_bytes(), &mut spans);
        let mut k: Vec<String> = spans.iter().map(|s| s.kind.to_string()).collect();
        k.sort();
        k.dedup();
        k
    }

    #[test]
    fn named_formats_are_caught() {
        let cases = [
            ("ghp_EXAMPLEfakeTOKEN0000aaaabbbbccccdddd", "github-pat"),
            ("AKIAFAKEEXAMPLE23456", "aws-access-token"),
            ("AKIAIOSFODNN7EXAMPLE", "aws-access-token"),
            ("sk_live_EXAMPLE0fake0key0000000000", "stripe-access-token"),
            ("rk_live_EXAMPLE0fake0key0000000000,", "stripe-access-token"),
            (
                "ya29.a0EXAMPLE_fake-google-oauth-token-0000",
                "google-oauth-token",
            ),
            (
                "xoxb-1234567890-1234567890123-EXAMPLEfakeSlackToken",
                "slack-bot-token",
            ),
            ("xoxp-EXAMPLE-fake-token", "slack-token"),
        ];
        for (text, kind) in cases {
            let got = kinds(&format!("value {text} here"));
            assert!(got.iter().any(|k| k == kind), "{text}: got {got:?}");
        }
    }

    #[test]
    fn keyword_prefilter_is_case_insensitive() {
        let rules = ProviderRules::load().unwrap();
        let on = rules.triggered(b"xxx AKIA yyy");
        assert!(on.iter().any(|&i| rules.rules[i].id == "aws-access-token"));
        let off = rules.triggered(b"nothing to see");
        assert!(!off.iter().any(|&i| rules.rules[i].id == "aws-access-token"));
    }
}
