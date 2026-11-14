//! Session capture, storage and search (docs/03). The pipeline:
//! transcript → redactor → summary → compress → encrypt → session store;
//! metadata and summary → index.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use inloop_core::Provenance;
use inloop_core::config::{
    Backend as BackendKind, PersonalConfig, personal_config_dir, personal_data_dir,
};
use inloop_index::{Doc, Kind};
use inloop_redact::Redacted;
use inloop_sessions::{AgentSource, HookEvent, SessionMeta, Summary, Transcript};
use inloop_store::{
    Backend, LocalBackend, ProjectKey, S3Backend, S3Config, SessionStore, key_path,
};
use serde::{Deserialize, Serialize};

use crate::project::{AppError, Project};

const RECORD_KIND: &str = "sessions";

/// The result of one capture.
#[derive(Clone, Debug, Serialize)]
pub struct CaptureOutcome {
    pub id: String,
    pub captured: bool,
    /// Why nothing was stored, when `captured` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    pub transcript_bytes: usize,
    pub redactions: BTreeMap<String, usize>,
}

/// A session as people and agents see it.
#[derive(Clone, Debug, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub meta: SessionMeta,
    pub provenance: Provenance,
    /// The redacted transcript, only when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

/// One line of the local capture log. Never holds transcript content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureLogEntry {
    pub at: String,
    pub session: String,
    pub event: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionStatus {
    pub store: String,
    pub key: String,
    pub key_created: bool,
    pub captured: usize,
    /// Transcripts the agent app has for this project that InLoop never stored.
    pub not_captured: Vec<String>,
    pub recent_failures: Vec<CaptureLogEntry>,
    pub hooks_installed: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct CaptureState {
    size: u64,
    modified_ns: u128,
    blob: String,
}

#[derive(Serialize, Deserialize)]
struct ActiveSession {
    session_id: String,
    since: String,
    #[serde(default)]
    transcript_path: Option<PathBuf>,
}

fn file_stamp(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((m.len(), t))
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Count `[REDACTED:<kind>]` markers.
fn count_markers(text: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut rest = text;
    while let Some(i) = rest.find("[REDACTED:") {
        let after = &rest[i + 10..];
        if let Some(end) = after.find(']') {
            *out.entry(after[..end].to_string()).or_insert(0) += 1;
            rest = &after[end..];
        } else {
            break;
        }
    }
    out
}

/// Store a redacted transcript. Taking `Redacted<Transcript>` is the gate:
/// there is no other way to get a transcript into the store.
fn store_transcript(
    store: &SessionStore,
    t: &Redacted<Transcript>,
) -> Result<(String, String), AppError> {
    let jsonl = t.get().to_jsonl();
    let hash = store
        .put_blob(jsonl.as_bytes())
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok((hash, jsonl))
}

/// Store redacted session metadata.
fn store_meta(store: &SessionStore, m: &Redacted<SessionMeta>) -> Result<(), AppError> {
    let json = serde_json::to_vec(m.get()).map_err(|e| AppError::Other(e.to_string()))?;
    store
        .put_record(RECORD_KIND, &m.get().id, &json)
        .map_err(|e| AppError::Other(e.to_string()))
}

impl Project {
    fn sessions_state(&self) -> PathBuf {
        self.state_dir().join("sessions")
    }

    /// The project's age key path.
    pub fn key_path(&self) -> Result<PathBuf, AppError> {
        let dir = personal_config_dir()
            .ok_or_else(|| AppError::Other("no home directory: set INLOOP_CONFIG_DIR".into()))?;
        Ok(key_path(&dir, &self.name))
    }

    /// The project key, created on first use.
    pub fn project_key(&self) -> Result<(ProjectKey, bool), AppError> {
        ProjectKey::load_or_create(&self.key_path()?).map_err(|e| AppError::Other(e.to_string()))
    }

    /// The session store for this project.
    pub fn store(&self) -> Result<SessionStore, AppError> {
        let personal = PersonalConfig::load()?;
        let backend: Box<dyn Backend> = match self.config.sessions.backend {
            BackendKind::Local => {
                let base = personal
                    .data_dir
                    .clone()
                    .or_else(personal_data_dir)
                    .ok_or_else(|| {
                        AppError::Other("no home directory: set INLOOP_DATA_DIR".into())
                    })?;
                Box::new(LocalBackend::new(base.join(safe_name(&self.name))))
            }
            BackendKind::S3 => {
                let s = &self.config.sessions;
                let bucket = s.bucket.clone().ok_or_else(|| {
                    AppError::Invalid(
                        "[sessions] backend = \"s3\" needs a bucket in inloop.toml".into(),
                    )
                })?;
                let creds = personal.s3.clone().unwrap_or_default();
                Box::new(
                    S3Backend::new(S3Config {
                        bucket,
                        endpoint: s.endpoint.clone(),
                        region: s.region.clone(),
                        prefix: s.prefix.clone().unwrap_or_else(|| safe_name(&self.name)),
                        access_key_id: creds.access_key_id,
                        secret_access_key: creds.secret_access_key.map(Into::into),
                    })
                    .map_err(|e| AppError::Other(e.to_string()))?,
                )
            }
        };
        let (key, _) = self.project_key()?;
        Ok(SessionStore::new(backend, key))
    }

    fn log_capture(&self, e: &CaptureLogEntry) {
        let path = self.sessions_state().join("capture-log.jsonl");
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let (Ok(mut f), Ok(line)) = (
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path),
            serde_json::to_string(e),
        ) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Capture a session from a hook event. Never panics; failures are
    /// logged (without content) and returned.
    pub fn capture(
        &self,
        ev: &HookEvent,
        source: &dyn AgentSource,
        force: bool,
    ) -> Result<CaptureOutcome, AppError> {
        let res = self.capture_inner(ev, source, force);
        self.log_capture(&CaptureLogEntry {
            at: inloop_core::now().to_string(),
            session: ev.session_id.clone(),
            event: ev.event.clone(),
            ok: res.is_ok(),
            error: res.as_ref().err().map(|e| e.to_string()),
        });
        res
    }

    fn capture_inner(
        &self,
        ev: &HookEvent,
        source: &dyn AgentSource,
        force: bool,
    ) -> Result<CaptureOutcome, AppError> {
        let id = safe_name(&ev.session_id);
        let path = ev
            .transcript_path
            .clone()
            .ok_or_else(|| AppError::Invalid("the hook gave no transcript_path".into()))?;
        let state_dir = self.sessions_state();
        std::fs::create_dir_all(state_dir.join("locks"))
            .map_err(|e| AppError::io("creating state dir", e))?;
        let state_path = state_dir.join(format!("{id}.json"));
        let stamp = file_stamp(&path);
        let previous: Option<CaptureState> = std::fs::read(&state_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if !force
            && let (Some(prev), Some((size, ns))) = (&previous, stamp)
            && prev.size == size
            && prev.modified_ns == ns
        {
            return Ok(CaptureOutcome {
                id,
                captured: false,
                skipped: Some("transcript unchanged since the last capture".into()),
                blob: Some(prev.blob.clone()),
                transcript_bytes: 0,
                redactions: BTreeMap::new(),
            });
        }

        // One capture per session at a time.
        let lock = state_dir.join("locks").join(format!("{id}.lock"));
        if let Ok(m) = std::fs::metadata(&lock)
            && m.modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(600))
        {
            let _ = std::fs::remove_file(&lock);
        }
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .is_err()
        {
            return Ok(CaptureOutcome {
                id,
                captured: false,
                skipped: Some("another capture of this session is running".into()),
                blob: None,
                transcript_bytes: 0,
                redactions: BTreeMap::new(),
            });
        }
        let result = self.capture_locked(ev, source, &id, &path, previous.map(|p| p.blob));
        let _ = std::fs::remove_file(&lock);
        let out = result?;
        if let (Some((size, ns)), Some(blob)) = (stamp, &out.blob) {
            let st = CaptureState {
                size,
                modified_ns: ns,
                blob: blob.clone(),
            };
            if let Ok(b) = serde_json::to_vec(&st) {
                let _ = std::fs::write(&state_path, b);
            }
        }
        Ok(out)
    }

    fn capture_locked(
        &self,
        ev: &HookEvent,
        source: &dyn AgentSource,
        id: &str,
        path: &Path,
        previous_blob: Option<String>,
    ) -> Result<CaptureOutcome, AppError> {
        let raw = source
            .read_transcript(path)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let before = count_markers(&raw);
        let transcript = Transcript::parse_jsonl(&raw);
        drop(raw);
        let redactor = self.redactor()?;
        // The gate: from here on only redacted data exists.
        let redacted = redactor
            .redact(transcript)
            .map_err(|e| AppError::Redactor(e.to_string()))?;
        let store = self.store()?;
        let (blob, jsonl) = store_transcript(&store, &redacted)?;
        let mut redactions = count_markers(&jsonl);
        for (k, n) in before {
            if let Some(v) = redactions.get_mut(&k) {
                *v = v.saturating_sub(n);
            }
        }
        redactions.retain(|_, v| *v > 0);

        let worktree = ev
            .cwd
            .as_ref()
            .and_then(|c| inloop_git::Repo::discover(c).ok())
            .map(|r| r.root)
            .unwrap_or_else(|| self.repo.root.clone());
        let summary = {
            let mut s = Summary::build(redacted.get(), Some(&worktree.to_string_lossy()));
            s.decisions = self.decisions_from_session(id);
            s
        };
        let (started, ended) = redacted.get().time_span();
        let wt_repo = inloop_git::Repo::discover(&worktree).ok();
        let branch = wt_repo
            .as_ref()
            .and_then(|r| r.current_branch().ok().flatten());
        let head = wt_repo
            .as_ref()
            .and_then(|r| r.head_commit().ok().flatten());
        let commits = self.commits_for_session(id);
        let meta = SessionMeta {
            id: id.to_string(),
            source: source.name().to_string(),
            project: self.name.clone(),
            task: self.task_for_branch(branch.as_deref()),
            branch,
            worktree: Some(worktree.to_string_lossy().into_owned()),
            started,
            ended,
            captured_at: inloop_core::now().to_string(),
            event: ev.event.clone(),
            head,
            commits,
            blob: blob.clone(),
            transcript_bytes: jsonl.len(),
            redactions: redactions.clone(),
            summary,
        };
        let meta = redactor
            .redact(meta)
            .map_err(|e| AppError::Redactor(e.to_string()))?;
        store_meta(&store, &meta)?;
        if let Some(old) = previous_blob.filter(|b| *b != blob) {
            let _ = store.delete_blob(&old);
        }
        self.index_session(meta.get())?;
        Ok(CaptureOutcome {
            id: id.to_string(),
            captured: true,
            skipped: None,
            blob: Some(blob),
            transcript_bytes: jsonl.len(),
            redactions,
        })
    }

    fn decisions_from_session(&self, id: &str) -> Vec<String> {
        self.decision_set(false)
            .map(|set| {
                set.entries
                    .iter()
                    .filter(|e| e.decision.session.as_deref() == Some(id))
                    .map(|e| e.decision.id.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn commits_for_session(&self, id: &str) -> Vec<String> {
        let grep = format!("--grep=^Session: {id}$");
        self.repo
            .try_run(&["log", "--all", "--format=%H", "-E", &grep])
            .ok()
            .flatten()
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Where a session's work stands: merged if its HEAD reached main.
    pub fn session_provenance(&self, m: &SessionMeta) -> Provenance {
        let main_ref = self.repo.main_ref(&self.main_branch).ok().flatten();
        let merged = match (&m.head, &main_ref) {
            (Some(h), Some(r)) => self.repo.is_ancestor(h, r).unwrap_or(false),
            _ => false,
        };
        if merged {
            return Provenance::merged();
        }
        let mut p = Provenance::unmerged(m.branch.clone(), m.worktree.as_ref().map(PathBuf::from));
        p.task = m
            .task
            .clone()
            .or_else(|| self.task_for_branch(m.branch.as_deref()));
        p
    }

    fn session_doc(&self, m: &SessionMeta) -> Doc {
        Doc {
            kind: Kind::Session,
            id: m.id.clone(),
            title: m.summary.title(),
            body: m.summary.search_text(),
            tags: [m.branch.clone().unwrap_or_default(), m.source.clone()].join(" "),
            meta: serde_json::json!({
                "started": m.started,
                "ended": m.ended,
                "branch": m.branch,
                "provenance": self.session_provenance(m),
            }),
        }
    }

    fn index_session(&self, m: &SessionMeta) -> Result<(), AppError> {
        self.index()?.upsert(&self.session_doc(m))?;
        Ok(())
    }

    fn load_meta(&self, store: &SessionStore, id: &str) -> Result<Option<SessionMeta>, AppError> {
        match store
            .get_record(RECORD_KIND, id)
            .map_err(|e| AppError::Other(e.to_string()))?
        {
            None => Ok(None),
            Some(b) => Ok(Some(serde_json::from_slice(&b).map_err(|e| {
                AppError::Other(format!("bad session record {id}: {e}"))
            })?)),
        }
    }

    /// Rebuild the session part of the index from the store: for a shared
    /// bucket, this is how other machines' sessions become searchable.
    pub fn session_sync(&self) -> Result<usize, AppError> {
        let store = self.store()?;
        let ids = store
            .list_records(RECORD_KIND)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut docs = Vec::new();
        for id in &ids {
            if let Some(m) = self.load_meta(&store, id)? {
                docs.push(self.session_doc(&m));
            }
        }
        self.index()?.replace_all(Kind::Session, &docs)?;
        Ok(docs.len())
    }

    pub fn session_list(&self) -> Result<Vec<SessionView>, AppError> {
        let store = self.store()?;
        let mut out = Vec::new();
        for id in store
            .list_records(RECORD_KIND)
            .map_err(|e| AppError::Other(e.to_string()))?
        {
            if let Some(meta) = self.load_meta(&store, &id)? {
                out.push(SessionView {
                    provenance: self.session_provenance(&meta),
                    meta,
                    transcript: None,
                });
            }
        }
        out.sort_by(|a, b| b.meta.started.cmp(&a.meta.started));
        Ok(out)
    }

    /// One session: its summary, and the redacted transcript if asked.
    pub fn session_get(&self, id: &str, transcript: bool) -> Result<SessionView, AppError> {
        let store = self.store()?;
        let id = self.resolve_session_id(&store, id)?;
        let meta = self
            .load_meta(&store, &id)?
            .ok_or_else(|| AppError::NotFound(format!("session {id}")))?;
        let transcript = if transcript {
            let bytes = store
                .get_blob(&meta.blob)
                .map_err(|e| AppError::Other(e.to_string()))?
                .ok_or_else(|| AppError::NotFound(format!("the transcript of session {id}")))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            None
        };
        Ok(SessionView {
            provenance: self.session_provenance(&meta),
            meta,
            transcript,
        })
    }

    /// Accept a unique prefix of a session ID.
    fn resolve_session_id(&self, store: &SessionStore, id: &str) -> Result<String, AppError> {
        let ids = store
            .list_records(RECORD_KIND)
            .map_err(|e| AppError::Other(e.to_string()))?;
        if ids.iter().any(|i| i == id) {
            return Ok(id.to_string());
        }
        let matches: Vec<&String> = ids.iter().filter(|i| i.starts_with(id)).collect();
        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(AppError::NotFound(format!("session {id}"))),
            _ => Err(AppError::Invalid(format!(
                "{id} matches {} sessions; give more of the ID",
                matches.len()
            ))),
        }
    }

    /// Delete a session completely: transcript, metadata, index entry.
    pub fn session_purge(&self, id: &str) -> Result<String, AppError> {
        let store = self.store()?;
        let id = self.resolve_session_id(&store, id)?;
        if let Some(meta) = self.load_meta(&store, &id)? {
            store
                .delete_blob(&meta.blob)
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        store
            .delete_record(RECORD_KIND, &id)
            .map_err(|e| AppError::Other(e.to_string()))?;
        self.index()?.remove(Kind::Session, &id)?;
        let _ = std::fs::remove_file(self.sessions_state().join(format!("{id}.json")));
        Ok(id)
    }

    pub fn session_status(&self, source: &dyn AgentSource) -> Result<SessionStatus, AppError> {
        let store = self.store()?;
        let (_, key_created) = self.project_key()?;
        let captured: Vec<String> = store
            .list_records(RECORD_KIND)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut not_captured = Vec::new();
        let mut dirs = Vec::new();
        for w in self.repo.worktrees()? {
            if let Some(d) = source.transcripts_dir(&w.path) {
                dirs.push(d);
            }
        }
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(id) = name.strip_suffix(".jsonl")
                    && !captured.iter().any(|c| c == id)
                {
                    not_captured.push(id.to_string());
                }
            }
        }
        not_captured.sort();
        let log = std::fs::read_to_string(self.sessions_state().join("capture-log.jsonl"))
            .unwrap_or_default();
        let mut failures: Vec<CaptureLogEntry> = log
            .lines()
            .filter_map(|l| serde_json::from_str::<CaptureLogEntry>(l).ok())
            .filter(|e| !e.ok)
            .collect();
        let n = failures.len();
        failures.drain(..n.saturating_sub(10));
        Ok(SessionStatus {
            store: store.describe(),
            key: self.key_path()?.display().to_string(),
            key_created,
            captured: captured.len(),
            not_captured,
            recent_failures: failures,
            hooks_installed: crate::hooks::claude_hooks_installed(&self.repo.root),
        })
    }

    fn active_path(&self, worktree: &Path) -> PathBuf {
        let canon = worktree
            .canonicalize()
            .unwrap_or_else(|_| worktree.to_path_buf());
        let name = inloop_sessions::ClaudeCode::project_slug(&canon);
        self.sessions_state()
            .join("active")
            .join(format!("{name}.json"))
    }

    /// Remember the session running in a worktree, for commit trailers and
    /// for linking decisions to sessions.
    pub fn mark_active(&self, ev: &HookEvent) -> Result<(), AppError> {
        let wt = ev.cwd.clone().unwrap_or_else(|| self.repo.root.clone());
        let wt = inloop_git::Repo::discover(&wt)
            .map(|r| r.root)
            .unwrap_or(wt);
        let path = self.active_path(&wt);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| AppError::io("creating state dir", e))?;
        }
        let a = ActiveSession {
            session_id: safe_name(&ev.session_id),
            since: inloop_core::now().to_string(),
            transcript_path: ev.transcript_path.clone(),
        };
        let b = serde_json::to_vec(&a).map_err(|e| AppError::Other(e.to_string()))?;
        std::fs::write(&path, b).map_err(|e| AppError::io("writing active session", e))
    }

    pub fn clear_active(&self, ev: &HookEvent) {
        let wt = ev.cwd.clone().unwrap_or_else(|| self.repo.root.clone());
        let wt = inloop_git::Repo::discover(&wt)
            .map(|r| r.root)
            .unwrap_or(wt);
        let path = self.active_path(&wt);
        if let Ok(b) = std::fs::read(&path)
            && let Ok(a) = serde_json::from_slice::<ActiveSession>(&b)
            && a.session_id == safe_name(&ev.session_id)
        {
            let _ = std::fs::remove_file(path);
        }
    }

    /// The captured agent session running in this worktree, if any:
    /// `INLOOP_SESSION_ID`, else the one the agent app's hooks registered.
    pub fn current_session(&self) -> Option<String> {
        if let Ok(s) = std::env::var("INLOOP_SESSION_ID")
            && !s.is_empty()
        {
            return Some(s);
        }
        let b = std::fs::read(self.active_path(&self.repo.root)).ok()?;
        let a: ActiveSession = serde_json::from_slice(&b).ok()?;
        // A session that hasn't been seen for half a day has probably ended
        // without telling us.
        let since: jiff::Timestamp = a.since.parse().ok()?;
        let age = inloop_core::now().duration_since(since);
        if age > jiff::SignedDuration::from_hours(12) {
            return None;
        }
        Some(a.session_id)
    }
}
