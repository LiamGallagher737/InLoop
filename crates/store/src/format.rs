//! The plaintext payload format inside each age file.
//!
//! ```text
//! magic    4 bytes  "INLS"
//! version  1 byte   1
//! kind     1 byte   'B' for a blob, 'R' for a record
//! name len 2 bytes  big-endian
//! name     n bytes  the record's storage key; empty for blobs
//! body     ...      one zstd frame of the caller's bytes
//! ```
//!
//! The whole thing is encrypted, so none of it is visible in the store. The
//! version byte lets the format evolve; readers refuse versions they do not
//! know rather than guessing. Binding a record's storage key into its
//! encrypted payload means someone with write access to the bucket cannot
//! swap one record for another without it being detected. Blobs need no such
//! binding: their storage key is the hash of their ciphertext.

use crate::StoreError;

const MAGIC: &[u8; 4] = b"INLS";
const VERSION: u8 = 1;
const ZSTD_LEVEL: i32 = 3;

/// What a payload holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Blob,
    Record,
}

impl Kind {
    fn byte(self) -> u8 {
        match self {
            Kind::Blob => b'B',
            Kind::Record => b'R',
        }
    }
}

/// Builds the plaintext payload: header, then `body` compressed.
pub(crate) fn encode(kind: Kind, name: &str, body: &[u8]) -> Result<Vec<u8>, StoreError> {
    let name_len = u16::try_from(name.len()).map_err(|_| StoreError::InvalidKey {
        key: name.to_owned(),
        reason: "it is too long",
    })?;
    let compressed = zstd::bulk::compress(body, ZSTD_LEVEL).map_err(|_| StoreError::Encrypt)?;
    let mut out = Vec::with_capacity(8 + name.len() + compressed.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(kind.byte());
    out.extend_from_slice(&name_len.to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// Checks the header against what was asked for and decompresses the body.
/// `what` names the blob or record in errors.
pub(crate) fn decode(
    kind: Kind,
    name: &str,
    payload: &[u8],
    what: &str,
) -> Result<Vec<u8>, StoreError> {
    let corrupt = |reason| StoreError::Corrupt {
        what: what.to_owned(),
        reason,
    };
    let rest = payload
        .strip_prefix(MAGIC)
        .ok_or_else(|| corrupt("missing InLoop header"))?;
    let (&version, rest) = rest
        .split_first()
        .ok_or_else(|| corrupt("truncated header"))?;
    if version != VERSION {
        return Err(corrupt("unknown format version"));
    }
    let (&kind_byte, rest) = rest
        .split_first()
        .ok_or_else(|| corrupt("truncated header"))?;
    if kind_byte != kind.byte() {
        return Err(corrupt("it is not the kind of object that was asked for"));
    }
    let (len, rest) = rest
        .split_at_checked(2)
        .ok_or_else(|| corrupt("truncated header"))?;
    let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
    let (stored_name, body) = rest
        .split_at_checked(len)
        .ok_or_else(|| corrupt("truncated header"))?;
    if stored_name != name.as_bytes() {
        return Err(corrupt(
            "it was written under a different name and moved here",
        ));
    }
    zstd::stream::decode_all(body).map_err(|_| corrupt("the compressed body is damaged"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() -> Result<(), StoreError> {
        let payload = encode(Kind::Record, "sessions/a.json.age", b"hello")?;
        assert!(payload.starts_with(b"INLS\x01R"));
        assert_eq!(
            decode(Kind::Record, "sessions/a.json.age", &payload, "x")?,
            b"hello"
        );
        let empty = encode(Kind::Blob, "", b"")?;
        assert_eq!(decode(Kind::Blob, "", &empty, "x")?, b"");
        Ok(())
    }

    #[test]
    fn rejects_mismatches() -> Result<(), StoreError> {
        let payload = encode(Kind::Record, "sessions/a.json.age", b"hello")?;
        let corrupt = |r: Result<Vec<u8>, StoreError>| matches!(r, Err(StoreError::Corrupt { .. }));
        assert!(corrupt(decode(
            Kind::Record,
            "sessions/b.json.age",
            &payload,
            "x"
        )));
        assert!(corrupt(decode(
            Kind::Blob,
            "sessions/a.json.age",
            &payload,
            "x"
        )));
        let mut future = payload.clone();
        future[4] = 2;
        assert!(corrupt(decode(
            Kind::Record,
            "sessions/a.json.age",
            &future,
            "x"
        )));
        assert!(corrupt(decode(
            Kind::Record,
            "sessions/a.json.age",
            &payload[..6],
            "x"
        )));
        assert!(corrupt(decode(
            Kind::Record,
            "sessions/a.json.age",
            b"junk",
            "x"
        )));
        let mut damaged = payload.clone();
        let last = damaged.len() - 1;
        damaged.truncate(last - 2);
        assert!(corrupt(decode(
            Kind::Record,
            "sessions/a.json.age",
            &damaged,
            "x"
        )));
        Ok(())
    }
}
