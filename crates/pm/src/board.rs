//! The board store: one JSON file per record under a board directory
//! (normally `<git common dir>/inloop/board`, docs/01):
//!
//! ```text
//! tasks/T-XXXX.json  claims/C-XXXX.json  reports/R-XXXX.json  blockers/B-XXXX.json
//! ```
//!
//! Writes only accept [`Redacted`] records (D-G1R6), and each is atomic: the
//! record is written to a temporary file in the same folder and renamed into
//! place. A corrupt file never stops a listing; it is returned on its own so
//! the caller can report it.

use std::io::Write;
use std::path::{Path, PathBuf};

use inloop_core::Id;
use inloop_redact::Redacted;

use crate::model::{Blocker, Claim, Record, RecordKind, Report, Task};

#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a valid board record: {message}")]
    Corrupt { path: PathBuf, message: String },
    #[error("could not serialise {id}: {message}")]
    Serialise { id: Id, message: String },
    #[error("{id} is not a {expected} ID")]
    WrongKind { id: Id, expected: &'static str },
    #[error(transparent)]
    Id(#[from] inloop_core::IdError),
}

/// A record file that could not be read or parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorruptRecord {
    pub path: PathBuf,
    pub message: String,
}

/// The records of one kind, plus any files that could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing<T> {
    /// Sorted by ID.
    pub records: Vec<T>,
    pub corrupt: Vec<CorruptRecord>,
}

/// A board directory.
#[derive(Clone, Debug)]
pub struct Board {
    dir: PathBuf,
}

impl Board {
    /// Open the board at `dir`. Nothing is created until the first write, so
    /// opening a board that doesn't exist yet lists as empty.
    pub fn open(dir: impl Into<PathBuf>) -> Board {
        Board { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, kind: RecordKind, id: &Id) -> PathBuf {
        self.dir.join(kind.folder()).join(format!("{id}.json"))
    }

    /// Whether a record of this kind and ID exists.
    pub fn exists(&self, kind: RecordKind, id: &Id) -> bool {
        self.path_for(kind, id).exists()
    }

    /// A fresh random ID for `kind` that no file on the board uses yet.
    pub fn new_id(&self, kind: RecordKind) -> Result<Id, BoardError> {
        Ok(kind.id_spec().generate_unused(|id| self.exists(kind, id))?)
    }

    // ----- reading -----

    fn get<T: Record>(&self, id: &Id) -> Result<Option<T>, BoardError> {
        let path = self.path_for(T::KIND, id);
        match std::fs::read_to_string(&path) {
            Ok(text) => parse_record::<T>(&path, &text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(BoardError::Read { path, source }),
        }
    }

    fn list<T: Record>(&self) -> Result<Listing<T>, BoardError> {
        let folder = self.dir.join(T::KIND.folder());
        let mut out = Listing {
            records: Vec::new(),
            corrupt: Vec::new(),
        };
        let entries = match std::fs::read_dir(&folder) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(source) => {
                return Err(BoardError::Read {
                    path: folder,
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| BoardError::Read {
                path: folder.clone(),
                source,
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| parse_record::<T>(&path, &text).map_err(|e| e.to_string()))
                .and_then(|rec| {
                    let stem = name.trim_end_matches(".json");
                    if rec.id().as_str() == stem {
                        Ok(rec)
                    } else {
                        Err(format!(
                            "file name says {stem} but the record is {}",
                            rec.id()
                        ))
                    }
                });
            match result {
                Ok(rec) => out.records.push(rec),
                Err(message) => out.corrupt.push(CorruptRecord { path, message }),
            }
        }
        out.records.sort_by(|a, b| a.id().cmp(b.id()));
        out.corrupt.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    pub fn list_tasks(&self) -> Result<Listing<Task>, BoardError> {
        self.list()
    }

    pub fn get_task(&self, id: &Id) -> Result<Option<Task>, BoardError> {
        self.get(id)
    }

    pub fn list_claims(&self) -> Result<Listing<Claim>, BoardError> {
        self.list()
    }

    /// Every claim on one task, open or released.
    pub fn claims_for_task(&self, task: &Id) -> Result<Vec<Claim>, BoardError> {
        Ok(self
            .list_claims()?
            .records
            .into_iter()
            .filter(|c| &c.task == task)
            .collect())
    }

    /// Every open claim on the board.
    pub fn open_claims(&self) -> Result<Vec<Claim>, BoardError> {
        Ok(self
            .list_claims()?
            .records
            .into_iter()
            .filter(Claim::is_open)
            .collect())
    }

    pub fn list_reports(&self) -> Result<Listing<Report>, BoardError> {
        self.list()
    }

    pub fn get_report(&self, id: &Id) -> Result<Option<Report>, BoardError> {
        self.get(id)
    }

    /// Reports for one task, oldest first.
    pub fn reports_for_task(&self, task: &Id) -> Result<Vec<Report>, BoardError> {
        let mut out: Vec<Report> = self
            .list_reports()?
            .records
            .into_iter()
            .filter(|r| &r.task == task)
            .collect();
        out.sort_by(|a, b| (&a.at, &a.id).cmp(&(&b.at, &b.id)));
        Ok(out)
    }

    pub fn list_blockers(&self) -> Result<Listing<Blocker>, BoardError> {
        self.list()
    }

    pub fn get_blocker(&self, id: &Id) -> Result<Option<Blocker>, BoardError> {
        self.get(id)
    }

    /// Blockers for one task, oldest first.
    pub fn blockers_for_task(&self, task: &Id) -> Result<Vec<Blocker>, BoardError> {
        let mut out: Vec<Blocker> = self
            .list_blockers()?
            .records
            .into_iter()
            .filter(|b| &b.task == task)
            .collect();
        out.sort_by(|a, b| (&a.at, &a.id).cmp(&(&b.at, &b.id)));
        Ok(out)
    }

    /// Blockers the PM hasn't folded into the board yet, oldest first.
    pub fn unfolded_blockers(&self) -> Result<Vec<Blocker>, BoardError> {
        let mut out: Vec<Blocker> = self
            .list_blockers()?
            .records
            .into_iter()
            .filter(|b| !b.folded)
            .collect();
        out.sort_by(|a, b| (&a.at, &a.id).cmp(&(&b.at, &b.id)));
        Ok(out)
    }

    // ----- writing: redacted records only (D-G1R6) -----

    fn put<T: Record>(&self, record: Redacted<T>) -> Result<PathBuf, BoardError> {
        let rec = record.get();
        let id = rec.id().clone();
        // The ID decides the file name, so check its shape before using it.
        if T::KIND.id_spec().parse(id.as_str()).is_err() {
            return Err(BoardError::WrongKind {
                id,
                expected: T::KIND.folder(),
            });
        }
        let mut text = serde_json::to_string_pretty(rec).map_err(|e| BoardError::Serialise {
            id: id.clone(),
            message: e.to_string(),
        })?;
        text.push('\n');
        let path = self.path_for(T::KIND, &id);
        write_atomic(&path, text.as_bytes())?;
        Ok(path)
    }

    /// Write a task (create or replace). Returns the file written.
    pub fn put_task(&self, task: Redacted<Task>) -> Result<PathBuf, BoardError> {
        self.put(task)
    }

    pub fn put_claim(&self, claim: Redacted<Claim>) -> Result<PathBuf, BoardError> {
        self.put(claim)
    }

    pub fn put_report(&self, report: Redacted<Report>) -> Result<PathBuf, BoardError> {
        self.put(report)
    }

    pub fn put_blocker(&self, blocker: Redacted<Blocker>) -> Result<PathBuf, BoardError> {
        self.put(blocker)
    }
}

fn parse_record<T: Record>(path: &Path, text: &str) -> Result<T, BoardError> {
    serde_json::from_str(text).map_err(|e| BoardError::Corrupt {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Write `bytes` to `path` via a temporary file in the same folder and a
/// rename, so a reader never sees half a record.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BoardError> {
    let err = |source| BoardError::Write {
        path: path.to_path_buf(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{name}.tmp-{}-{nanos}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockerKind, TaskStatus};
    use inloop_redact::{PatternRule, Redactor, RedactorConfig};

    fn redactor(root: &Path) -> Redactor {
        let config = RedactorConfig {
            root: root.to_path_buf(),
            known_value_files: vec![".env".into()],
            patterns: vec![PatternRule {
                kind: "test-token".into(),
                regex: r"tok_test_[A-Za-z0-9]{12,}".into(),
            }],
            personal_data: false,
            include_process_env: false,
        };
        match Redactor::new(&config) {
            Ok(r) => r,
            Err(e) => unreachable!("redactor: {e}"),
        }
    }

    fn task(id: &str, description: &str) -> Task {
        Task {
            id: Id::parse_loose(id).unwrap_or_else(|_| unreachable!()),
            title: "Add the thing".into(),
            description: description.into(),
            status: TaskStatus::Ready,
            blocked_by: vec![],
            blocked_reason: None,
            touches: vec!["src/**".into()],
            branch: None,
            worktree: None,
            session: None,
            owner: Some("sam".into()),
            decisions: vec![],
            notes: vec![],
            created: "2026-11-14T00:00:00Z".into(),
            updated: "2026-11-14T00:00:00Z".into(),
        }
    }

    #[test]
    fn round_trip_redacts_secrets_on_disk() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let secret = "sk_live_9fK2mQ7xR4tY8wZ1pL6nB3vC";
        std::fs::write(root.path().join(".env"), format!("PAYMENTS_KEY={secret}\n"))?;
        let r = redactor(root.path());
        let board = Board::open(root.path().join("board"));

        let t = task(
            "T-4M1P",
            &format!("Use {secret} and tok_test_ABCDEFGHJKMN12 to call the API"),
        );
        let path = board.put_task(r.redact(t.clone())?)?;
        assert_eq!(path, root.path().join("board/tasks/T-4M1P.json"));

        let on_disk = std::fs::read_to_string(&path)?;
        assert!(!on_disk.contains(secret), "{on_disk}");
        assert!(!on_disk.contains("tok_test_ABCDEFGHJKMN12"), "{on_disk}");
        assert!(on_disk.contains("[REDACTED"), "{on_disk}");

        let back = board.get_task(&t.id)?.ok_or("missing")?;
        assert_eq!(back.id, t.id);
        assert_eq!(back.title, t.title);
        assert!(!back.description.contains(secret));
        let listing = board.list_tasks()?;
        assert_eq!(listing.records, vec![back]);
        assert!(listing.corrupt.is_empty());
        // No temporary files are left behind.
        let names: Vec<_> = std::fs::read_dir(root.path().join("board/tasks"))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1);
        Ok(())
    }

    #[test]
    fn missing_board_lists_empty() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let board = Board::open(root.path().join("nope"));
        assert!(board.list_tasks()?.records.is_empty());
        assert!(board.unfolded_blockers()?.is_empty());
        assert_eq!(board.get_task(&Id::parse_loose("T-AAAA")?)?, None);
        assert!(!root.path().join("nope").exists());
        Ok(())
    }

    #[test]
    fn corrupt_files_are_reported_not_fatal() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let r = redactor(root.path());
        let board = Board::open(root.path());
        board.put_task(r.redact(task("T-AAAA", "fine"))?)?;
        std::fs::write(root.path().join("tasks/T-BBBB.json"), "{ not json")?;
        // A record whose ID doesn't match its file name.
        let moved = serde_json::to_string(&task("T-CCCC", "x"))?;
        std::fs::write(root.path().join("tasks/T-DDDD.json"), moved)?;
        std::fs::write(root.path().join("tasks/README.txt"), "ignored")?;

        let listing = board.list_tasks()?;
        assert_eq!(listing.records.len(), 1);
        assert_eq!(listing.corrupt.len(), 2);
        assert!(listing.corrupt[0].path.ends_with("T-BBBB.json"));
        assert!(listing.corrupt[1].message.contains("T-CCCC"));
        assert!(matches!(
            board.get_task(&Id::parse_loose("T-BBBB")?),
            Err(BoardError::Corrupt { .. })
        ));
        Ok(())
    }

    #[test]
    fn new_ids_skip_existing_files() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let board = Board::open(root.path());
        let id = board.new_id(RecordKind::Claim)?;
        assert!(id.as_str().starts_with("C-"));
        assert!(!board.exists(RecordKind::Claim, &id));
        Ok(())
    }

    #[test]
    fn writes_refuse_ids_of_the_wrong_kind() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let r = redactor(root.path());
        let board = Board::open(root.path());
        let bad = task("D-AAAA", "x");
        assert!(matches!(
            board.put_task(r.redact(bad)?),
            Err(BoardError::WrongKind { .. })
        ));
        Ok(())
    }

    #[test]
    fn reports_blockers_and_claims_by_task() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let r = redactor(root.path());
        let board = Board::open(root.path());
        let t1 = Id::parse_loose("T-AAAA")?;
        let t2 = Id::parse_loose("T-BBBB")?;
        for (id, task, at, folded) in [
            ("B-0001", &t1, "2026-11-14T02:00:00Z", false),
            ("B-0002", &t1, "2026-11-14T01:00:00Z", true),
            ("B-0003", &t2, "2026-11-14T03:00:00Z", false),
        ] {
            board.put_blocker(r.redact(Blocker {
                id: Id::parse_loose(id)?,
                task: task.clone(),
                at: at.into(),
                session: None,
                kind: BlockerKind::Human,
                on: "a question".into(),
                details: String::new(),
                folded,
            })?)?;
        }
        board.put_report(r.redact(Report {
            id: Id::parse_loose("R-0001")?,
            task: t2.clone(),
            at: "2026-11-14T04:00:00Z".into(),
            session: None,
            done: vec!["it".into()],
            not_done: vec![],
            checks: vec![],
            decisions: vec![],
            plan_changes: vec![],
            summary: "done".into(),
        })?)?;
        board.put_claim(r.redact(Claim {
            id: Id::parse_loose("C-0001")?,
            task: t1.clone(),
            owner: "sam".into(),
            claimed_at: "2026-11-14T00:00:00Z".into(),
            released_at: None,
        })?)?;

        let ids = |bs: Vec<Blocker>| bs.into_iter().map(|b| b.id.to_string()).collect::<Vec<_>>();
        assert_eq!(ids(board.blockers_for_task(&t1)?), ["B-0002", "B-0001"]);
        assert_eq!(ids(board.unfolded_blockers()?), ["B-0001", "B-0003"]);
        assert_eq!(board.reports_for_task(&t2)?.len(), 1);
        assert!(board.reports_for_task(&t1)?.is_empty());
        assert_eq!(board.claims_for_task(&t1)?.len(), 1);
        assert_eq!(board.open_claims()?.len(), 1);
        Ok(())
    }
}
