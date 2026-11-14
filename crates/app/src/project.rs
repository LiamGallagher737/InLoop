//! Opening a project: the repo, its config, the redactor and the index.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use inloop_core::ProjectConfig;
use inloop_git::Repo;
use inloop_index::Index;
use inloop_redact::{PatternRule, Redactor, RedactorConfig};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Git(#[from] inloop_git::GitError),
    #[error(transparent)]
    Config(#[from] inloop_core::config::ConfigError),
    #[error(transparent)]
    Index(#[from] inloop_index::IndexError),
    #[error("could not set up redaction, so nothing will be written: {0}")]
    Redactor(String),
    #[error(transparent)]
    Op(#[from] inloop_decisions::ops::OpError),
    #[error("{0}")]
    Invalid(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{what}: {source}")]
    Io {
        what: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn io(what: impl Into<String>, source: std::io::Error) -> Self {
        AppError::Io {
            what: what.into(),
            source,
        }
    }
}

/// An open project.
pub struct Project {
    pub repo: Repo,
    pub config: ProjectConfig,
    pub main_branch: String,
    pub name: String,
    redactor: OnceLock<Result<Redactor, String>>,
}

/// Where the caller is: for the `context` tool and `inloop status`.
#[derive(Clone, Debug, Serialize)]
pub struct Context {
    pub project: String,
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub branch: Option<String>,
    pub main_branch: String,
    pub on_main: bool,
    pub ahead_of_main: Option<u32>,
    pub behind_main: Option<u32>,
    pub worktrees: Vec<WorktreeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub current: bool,
}

impl Project {
    /// Open the project containing `dir`.
    pub fn open(dir: &Path) -> Result<Project, AppError> {
        let repo = Repo::discover(dir)?;
        let config = ProjectConfig::load(&repo.root)?;
        let main_branch = repo.main_branch(config.project.main_branch.as_deref())?;
        let name = config.project_name(&main_root(&repo).unwrap_or_else(|| repo.root.clone()));
        Ok(Project {
            repo,
            config,
            main_branch,
            name,
            redactor: OnceLock::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.repo.root
    }

    /// The decisions directory of the current worktree.
    pub fn decisions_dir(&self) -> PathBuf {
        self.repo.root.join(&self.config.decisions.dir)
    }

    /// The redactor, built once. Every write goes through it (D-G1R6).
    pub fn redactor(&self) -> Result<&Redactor, AppError> {
        let r = self.redactor.get_or_init(|| {
            let cfg = RedactorConfig {
                root: self.repo.root.clone(),
                known_value_files: self.config.redaction.known_value_files.clone(),
                patterns: self
                    .config
                    .redaction
                    .patterns
                    .iter()
                    .map(|p| PatternRule {
                        kind: p.kind.clone(),
                        regex: p.regex.clone(),
                    })
                    .collect(),
                personal_data: self.config.redaction.personal_data,
                include_process_env: true,
            };
            Redactor::new(&cfg).map_err(|e| e.to_string())
        });
        r.as_ref().map_err(|e| AppError::Redactor(e.clone()))
    }

    /// The index, opened fresh (cheap) so callers in different threads don't
    /// share a connection.
    pub fn index(&self) -> Result<Index, AppError> {
        Ok(Index::open(&self.repo.inloop_dir().join("index.sqlite"))?)
    }

    /// The local state directory in the git common dir (D-SRGY).
    pub fn state_dir(&self) -> PathBuf {
        self.repo.inloop_dir()
    }

    /// The PM task that owns `branch`, when the PM layer is on. Filled in
    /// by the PM layer; `None` otherwise.
    pub fn task_for_branch(&self, _branch: Option<&str>) -> Option<String> {
        None
    }

    pub fn context(&self) -> Result<Context, AppError> {
        let branch = self.repo.current_branch()?;
        let main_ref = self.repo.main_ref(&self.main_branch)?;
        let (ahead, behind) = match (&main_ref, self.repo.head_commit()?) {
            (Some(m), Some(_)) => match self.repo.ahead_behind(m, "HEAD") {
                Ok((a, b)) => (Some(a), Some(b)),
                Err(_) => (None, None),
            },
            _ => (None, None),
        };
        let here = self
            .repo
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.repo.root.clone());
        let worktrees = self
            .repo
            .worktrees()?
            .into_iter()
            .filter(|w| !w.bare)
            .map(|w| WorktreeInfo {
                current: w.path.canonicalize().unwrap_or_else(|_| w.path.clone()) == here,
                path: w.path,
                branch: w.branch,
            })
            .collect();
        Ok(Context {
            project: self.name.clone(),
            repo: main_root(&self.repo).unwrap_or_else(|| self.repo.root.clone()),
            worktree: self.repo.root.clone(),
            on_main: branch.as_deref() == Some(self.main_branch.as_str()),
            task: self.task_for_branch(branch.as_deref()),
            branch,
            main_branch: self.main_branch.clone(),
            ahead_of_main: ahead,
            behind_main: behind,
            worktrees,
            session: None,
        })
    }

    /// The git user name, for `decided_by` and `promoted_by` from the CLI.
    pub fn git_user(&self) -> Option<String> {
        self.repo
            .try_run(&["config", "user.name"])
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// The main worktree's path: the parent of the common dir for a normal clone.
fn main_root(repo: &Repo) -> Option<PathBuf> {
    repo.worktrees()
        .ok()?
        .into_iter()
        .find(|w| w.is_main && !w.bare)
        .map(|w| w.path)
}
