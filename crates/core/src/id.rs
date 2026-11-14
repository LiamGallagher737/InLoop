//! Random Crockford base32 IDs (D-11JK, D-P5PT).
//!
//! An ID is a prefix, a dash and a fixed number of Crockford base32
//! characters, for example `D-7KQ2` or `T-4M1P`. IDs are generated randomly
//! where they are needed, never allocated, and never change once created.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The Crockford base32 alphabet: no I, L, O or U.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The shape of an ID: its prefix and how many characters follow the dash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdSpec {
    pub prefix: String,
    pub length: usize,
}

impl IdSpec {
    pub fn new(prefix: impl Into<String>, length: usize) -> Self {
        Self {
            prefix: prefix.into(),
            length,
        }
    }

    /// The default for decisions: `D-XXXX`.
    pub fn decisions() -> Self {
        Self::new("D", 4)
    }

    /// The default for PM tasks: `T-XXXX`.
    pub fn tasks() -> Self {
        Self::new("T", 4)
    }

    /// A new random ID. Never all digits, so it can't clash with an old
    /// sequential number kept as an alias (D-P5PT).
    pub fn generate(&self) -> Id {
        loop {
            let body: String = (0..self.length)
                .map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char)
                .collect();
            if self.length == 0 || !body.bytes().all(|b| b.is_ascii_digit()) {
                return Id(format!("{}-{}", self.prefix, body));
            }
        }
    }

    /// A new random ID that `taken` says is free. Gives up after many tries,
    /// which only happens when the ID space is nearly full.
    pub fn generate_unused(&self, taken: impl Fn(&Id) -> bool) -> Result<Id, IdError> {
        for _ in 0..10_000 {
            let id = self.generate();
            if !taken(&id) {
                return Ok(id);
            }
        }
        Err(IdError::Exhausted {
            prefix: self.prefix.clone(),
            length: self.length,
        })
    }

    /// Parse `text` as an ID of this shape, normalising case and Crockford
    /// look-alikes (`I`/`L` read as `1`, `O` as `0`).
    pub fn parse(&self, text: &str) -> Result<Id, IdError> {
        let id = Id::parse_loose(text)?;
        let (prefix, body) = id.split();
        if !prefix.eq_ignore_ascii_case(&self.prefix) || body.len() != self.length {
            return Err(IdError::WrongShape {
                text: text.to_string(),
                expected: format!("{}-{}", self.prefix, "X".repeat(self.length)),
            });
        }
        Ok(id)
    }
}

/// An ID such as `D-7KQ2`. Always stored in canonical form: upper case, no
/// look-alike characters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl Id {
    /// Parse any `PREFIX-BODY` ID, where the prefix is letters and the body
    /// is Crockford base32 (with look-alikes normalised).
    pub fn parse_loose(text: &str) -> Result<Id, IdError> {
        let text = text.trim();
        let Some((prefix, body)) = text.split_once('-') else {
            return Err(IdError::Invalid(text.to_string()));
        };
        if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_alphabetic()) || body.is_empty()
        {
            return Err(IdError::Invalid(text.to_string()));
        }
        let mut canonical = String::with_capacity(body.len());
        for c in body.chars() {
            canonical.push(normalise_char(c).ok_or_else(|| IdError::Invalid(text.to_string()))?);
        }
        Ok(Id(format!("{}-{}", prefix.to_ascii_uppercase(), canonical)))
    }

    /// The canonical text, for example `D-7KQ2`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The prefix and the body, split at the first dash.
    pub fn split(&self) -> (&str, &str) {
        self.0.split_once('-').unwrap_or((&self.0, ""))
    }
}

fn normalise_char(c: char) -> Option<char> {
    let up = c.to_ascii_uppercase();
    match up {
        'I' | 'L' => Some('1'),
        'O' => Some('0'),
        _ if ALPHABET.contains(&(up as u8)) && up.is_ascii() => Some(up),
        _ => None,
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Id {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Id::parse_loose(s)
    }
}

impl TryFrom<String> for Id {
    type Error = IdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Id::parse_loose(&value)
    }
}

impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("`{0}` is not an ID: expected something like D-7KQ2")]
    Invalid(String),
    #[error("`{text}` is not the right shape: expected {expected}")]
    WrongShape { text: String, expected: String },
    #[error(
        "could not find a free {prefix}- ID of length {length}; raise id_length in inloop.toml"
    )]
    Exhausted { prefix: String, length: usize },
}

/// Find every reference to an ID of this shape in `text`, as written (not
/// normalised), with byte offsets. Used by lint to check references.
pub fn find_references<'a>(spec: &IdSpec, text: &'a str) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let prefix = spec.prefix.as_bytes();
    let mut i = 0;
    while i + prefix.len() < bytes.len() {
        let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if starts_word
            && bytes[i..].starts_with(prefix)
            && bytes.get(i + prefix.len()) == Some(&b'-')
        {
            let body_start = i + prefix.len() + 1;
            let mut end = body_start;
            while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
                end += 1;
            }
            let body = &text[body_start..end];
            if body.len() == spec.length
                && body
                    .bytes()
                    .all(|b| ALPHABET.contains(&b) && !b.is_ascii_lowercase())
            {
                out.push((i, &text[i..end]));
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_the_right_shape() {
        let spec = IdSpec::decisions();
        for _ in 0..2_000 {
            let id = spec.generate();
            let (p, body) = id.split();
            assert_eq!(p, "D");
            assert_eq!(body.len(), 4);
            assert!(body.bytes().all(|b| ALPHABET.contains(&b)));
            assert!(!body.bytes().all(|b| b.is_ascii_digit()), "{id}");
            assert_eq!(spec.parse(id.as_str()), Ok(id.clone()));
        }
    }

    #[test]
    fn parsing_normalises_case_and_lookalikes() {
        let spec = IdSpec::decisions();
        assert_eq!(spec.parse("d-7kq2").map(|i| i.0), Ok("D-7KQ2".into()));
        assert_eq!(spec.parse("D-ILO2").map(|i| i.0), Ok("D-1102".into()));
        assert!(spec.parse("D-7KQ").is_err());
        assert!(spec.parse("T-7KQ2").is_err());
        assert!(spec.parse("D-7KU2").is_err());
        assert!(Id::parse_loose("nonsense").is_err());
    }

    #[test]
    fn old_sequential_numbers_parse_loosely() {
        assert_eq!(Id::parse_loose("D-264").map(|i| i.0), Ok("D-264".into()));
    }

    #[test]
    fn generate_unused_skips_taken_ids() {
        let spec = IdSpec::new("D", 1);
        let id = spec.generate_unused(|i| i.as_str() != "D-Z");
        assert_eq!(id.map(|i| i.0), Ok("D-Z".into()));
        let none = IdSpec::new("D", 1).generate_unused(|_| true);
        assert!(matches!(none, Err(IdError::Exhausted { .. })));
    }

    #[test]
    fn finds_references_in_text() {
        let spec = IdSpec::decisions();
        let text = "See D-7KQ2 and (D-0217), not XD-7KQ2, D-7KQ22 or D-264.";
        let refs: Vec<_> = find_references(&spec, text)
            .into_iter()
            .map(|r| r.1)
            .collect();
        assert_eq!(refs, vec!["D-7KQ2", "D-0217"]);
    }
}
