//! The local SQLite FTS5 index (D-SRGY). It lives in the git common
//! directory, is shared by every worktree of a clone, and is derived data:
//! decisions are re-indexed from their files on every refresh, and session
//! summaries can be re-indexed from the session store.
#![forbid(unsafe_code)]

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("search index error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not create the index directory {path}: {source}")]
    Dir {
        path: String,
        source: std::io::Error,
    },
    #[error("bad stored data in the index: {0}")]
    Json(#[from] serde_json::Error),
}

/// What a document is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Decision,
    Session,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Session => "session",
        }
    }
}

/// One searchable document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub body: String,
    /// Space-separated tags and aliases.
    pub tags: String,
    /// Anything the caller wants back with a hit (provenance, status...).
    pub meta: serde_json::Value,
}

/// A search hit, best first.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    /// A short excerpt with the matching terms marked `[like this]`.
    pub snippet: String,
    /// Lower is better (bm25).
    pub rank: f64,
    pub meta: serde_json::Value,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open (or create) the index at `path`.
    pub fn open(path: &Path) -> Result<Index, IndexError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| IndexError::Dir {
                path: dir.display().to_string(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// An index in memory, for tests and one-off use.
    pub fn in_memory() -> Result<Index, IndexError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Index, IndexError> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            // Derived data: on any schema change, start again.
            conn.execute_batch(
                "DROP TABLE IF EXISTS docs;
                 DROP TABLE IF EXISTS docs_fts;
                 DROP TABLE IF EXISTS state;",
            )?;
            conn.execute_batch(
                "CREATE TABLE docs (
                    rowid INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL,
                    id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    body TEXT NOT NULL,
                    tags TEXT NOT NULL,
                    meta TEXT NOT NULL,
                    UNIQUE (kind, id)
                 );
                 CREATE VIRTUAL TABLE docs_fts USING fts5(
                    id, title, body, tags,
                    content='docs', content_rowid='rowid',
                    tokenize='porter unicode61'
                 );
                 CREATE TRIGGER docs_ai AFTER INSERT ON docs BEGIN
                    INSERT INTO docs_fts(rowid, id, title, body, tags)
                    VALUES (new.rowid, new.id, new.title, new.body, new.tags);
                 END;
                 CREATE TRIGGER docs_ad AFTER DELETE ON docs BEGIN
                    INSERT INTO docs_fts(docs_fts, rowid, id, title, body, tags)
                    VALUES ('delete', old.rowid, old.id, old.title, old.body, old.tags);
                 END;
                 CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Index { conn })
    }

    /// Replace every document of `kind` with `docs`, in one transaction.
    pub fn replace_all(&mut self, kind: Kind, docs: &[Doc]) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM docs WHERE kind = ?1", params![kind.as_str()])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO docs (kind, id, title, body, tags, meta) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for d in docs.iter().filter(|d| d.kind == kind) {
                stmt.execute(params![
                    kind.as_str(),
                    d.id,
                    d.title,
                    d.body,
                    d.tags,
                    serde_json::to_string(&d.meta)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Add or replace one document.
    pub fn upsert(&mut self, doc: &Doc) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM docs WHERE kind = ?1 AND id = ?2",
            params![doc.kind.as_str(), doc.id],
        )?;
        tx.execute(
            "INSERT INTO docs (kind, id, title, body, tags, meta) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                doc.kind.as_str(),
                doc.id,
                doc.title,
                doc.body,
                doc.tags,
                serde_json::to_string(&doc.meta)?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Remove one document.
    pub fn remove(&mut self, kind: Kind, id: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "DELETE FROM docs WHERE kind = ?1 AND id = ?2",
            params![kind.as_str(), id],
        )?;
        Ok(())
    }

    /// One document by kind and ID.
    pub fn get(&self, kind: Kind, id: &str) -> Result<Option<Doc>, IndexError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, body, tags, meta FROM docs WHERE kind = ?1 AND id = ?2",
                params![kind.as_str(), id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((id, title, body, tags, meta)) => Ok(Some(Doc {
                kind,
                id,
                title,
                body,
                tags,
                meta: serde_json::from_str(&meta)?,
            })),
        }
    }

    /// Every document of `kind`, ordered by ID.
    pub fn all(&self, kind: Kind) -> Result<Vec<Doc>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, body, tags, meta FROM docs WHERE kind = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![kind.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, title, body, tags, meta) = row?;
            out.push(Doc {
                kind,
                id,
                title,
                body,
                tags,
                meta: serde_json::from_str(&meta)?,
            });
        }
        Ok(out)
    }

    /// Full-text search. Every word must match (as a prefix); if nothing
    /// does, any word may. `kinds` empty means all kinds.
    pub fn search(
        &self,
        query: &str,
        kinds: &[Kind],
        limit: usize,
    ) -> Result<Vec<Hit>, IndexError> {
        let terms = terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let all = terms.join(" ");
        let mut hits = self.run_search(&all, kinds, limit)?;
        if hits.is_empty() && terms.len() > 1 {
            hits = self.run_search(&terms.join(" OR "), kinds, limit)?;
        }
        Ok(hits)
    }

    fn run_search(&self, fts: &str, kinds: &[Kind], limit: usize) -> Result<Vec<Hit>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT d.kind, d.id, d.title,
                    snippet(docs_fts, 2, '[', ']', ' … ', 16),
                    bm25(docs_fts, 10.0, 5.0, 1.0, 3.0),
                    d.meta
             FROM docs_fts JOIN docs d ON d.rowid = docs_fts.rowid
             WHERE docs_fts MATCH ?1
             ORDER BY bm25(docs_fts, 10.0, 5.0, 1.0, 3.0)
             LIMIT ?2",
        )?;
        // Over-fetch so filtering by kind still fills the limit.
        let fetch = if kinds.is_empty() {
            limit
        } else {
            limit * 4 + 20
        };
        let rows = stmt.query_map(params![fts, fetch as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, id, title, snippet, rank, meta) = row?;
            let kind = if kind == "session" {
                Kind::Session
            } else {
                Kind::Decision
            };
            if !kinds.is_empty() && !kinds.contains(&kind) {
                continue;
            }
            out.push(Hit {
                kind,
                id,
                title,
                snippet,
                rank,
                meta: serde_json::from_str(&meta)?,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// A small key-value store for bookkeeping.
    pub fn get_state(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_state(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT INTO state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

/// Split a query into quoted FTS5 prefix terms, so user text can never be
/// read as FTS5 syntax.
fn terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(kind: Kind, id: &str, title: &str, body: &str) -> Doc {
        Doc {
            kind,
            id: id.into(),
            title: title.into(),
            body: body.into(),
            tags: String::new(),
            meta: serde_json::json!({"x": id}),
        }
    }

    #[test]
    fn searches_and_ranks() {
        let mut idx = Index::in_memory().unwrap();
        idx.replace_all(
            Kind::Decision,
            &[
                doc(
                    Kind::Decision,
                    "D-AAAA",
                    "Snap writes go through one helper",
                    "The confirm check lives in one place.",
                ),
                doc(
                    Kind::Decision,
                    "D-BBBB",
                    "Use age for encryption",
                    "Transcripts are encrypted with age keys.",
                ),
            ],
        )
        .unwrap();
        idx.upsert(&doc(
            Kind::Session,
            "s1",
            "Session about encryption",
            "We discussed encrypting transcripts.",
        ))
        .unwrap();

        let hits = idx.search("encrypt", &[], 10).unwrap();
        assert_eq!(hits.len(), 2);
        let hits = idx.search("encrypt", &[Kind::Decision], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "D-BBBB");
        assert_eq!(hits[0].meta["x"], "D-BBBB");

        // AND first, then OR.
        let hits = idx
            .search("snap encryption", &[Kind::Decision], 10)
            .unwrap();
        assert_eq!(hits.len(), 2);
        // FTS syntax in user input is harmless.
        assert!(idx.search("\"unbalanced OR (", &[], 10).is_ok());
        // IDs are searchable.
        let hits = idx.search("D-AAAA", &[], 10).unwrap();
        assert_eq!(hits.first().map(|h| h.id.as_str()), Some("D-AAAA"));

        // Replacing decisions leaves sessions alone.
        idx.replace_all(Kind::Decision, &[]).unwrap();
        assert_eq!(idx.search("encrypt", &[], 10).unwrap().len(), 1);
        assert!(idx.get(Kind::Session, "s1").unwrap().is_some());
        idx.remove(Kind::Session, "s1").unwrap();
        assert!(idx.all(Kind::Session).unwrap().is_empty());
    }

    #[test]
    fn persists_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/index.sqlite");
        {
            let mut idx = Index::open(&path).unwrap();
            idx.upsert(&doc(Kind::Session, "s1", "t", "hello world"))
                .unwrap();
            idx.set_state("k", "v").unwrap();
        }
        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.search("hello", &[], 5).unwrap().len(), 1);
        assert_eq!(idx.get_state("k").unwrap().as_deref(), Some("v"));
    }
}
