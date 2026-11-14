//! Repository and worktree discovery, and reading files from branches,
//! through the `git` command line (D-CXS5).
//!
//! InLoop never assumes where worktrees live: it asks git. Files on
//! branches that aren't checked out anywhere are read straight from git
//! objects.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("{0} is not inside a git repository; run InLoop from a repo, or `git init` first")]
    NotARepo(PathBuf),
    #[error("could not run git: {0}; is git installed and on PATH?")]
    Spawn(std::io::Error),
    #[error("`git {args}` failed: {stderr}")]
    Failed { args: String, stderr: String },
    #[error("unexpected output from `git {args}`: {detail}")]
    Parse { args: String, detail: String },
}

/// A checked-out worktree of the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The commit checked out, if any.
    pub head: Option<String>,
    /// The short branch name (`snap-writes`), or `None` when detached.
    pub branch: Option<String>,
    pub bare: bool,
    /// True for the main worktree (the first one git lists).
    pub is_main: bool,
}

/// A branch ref and the commit it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRef {
    /// Full ref name, for example `refs/remotes/origin/snap-writes`.
    pub refname: String,
    /// Short name for people, for example `origin/snap-writes`.
    pub short: String,
    pub commit: String,
}

/// A file in a tree: its path relative to the tree root and its blob ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFile {
    pub name: String,
    pub blob: String,
}

/// One repository, opened from any directory inside any of its worktrees.
#[derive(Clone, Debug)]
pub struct Repo {
    /// Top of the worktree we were opened from.
    pub root: PathBuf,
    /// The git common directory, shared by every worktree of the clone.
    pub common_dir: PathBuf,
}

impl Repo {
    /// Find the repository containing `start`.
    pub fn discover(start: &Path) -> Result<Repo, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(start)
            .args(["rev-parse", "--show-toplevel", "--git-common-dir"])
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::NotARepo(start.to_path_buf()));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let (Some(top), Some(common)) = (lines.next(), lines.next()) else {
            return Err(GitError::NotARepo(start.to_path_buf()));
        };
        let root = PathBuf::from(top);
        let common = PathBuf::from(common);
        let common_dir = if common.is_absolute() {
            common
        } else {
            start.join(common)
        };
        let common_dir = common_dir.canonicalize().unwrap_or(common_dir);
        Ok(Repo { root, common_dir })
    }

    /// Where InLoop keeps the index and local state for this clone (D-SRGY).
    pub fn inloop_dir(&self) -> PathBuf {
        self.common_dir.join("inloop")
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.root);
        // Never prompt, never page, never colour.
        c.env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat");
        c.args(["-c", "color.ui=false"]);
        c
    }

    /// Run git with `args` and return stdout as text.
    pub fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.cmd().args(args).output().map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::Failed {
                args: args.join(" "),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Like `run`, but `Ok(None)` when git exits non-zero.
    pub fn try_run(&self, args: &[&str]) -> Result<Option<String>, GitError> {
        let out = self.cmd().args(args).output().map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    /// The current branch of the worktree we were opened from, or `None`
    /// when detached.
    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        Ok(self
            .try_run(&["symbolic-ref", "--quiet", "--short", "HEAD"])?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    /// The commit HEAD points at, if there is one yet.
    pub fn head_commit(&self) -> Result<Option<String>, GitError> {
        Ok(self
            .try_run(&["rev-parse", "--verify", "--quiet", "HEAD"])?
            .map(|s| s.trim().to_string()))
    }

    /// The main branch: `configured` if given, else the branch `origin/HEAD`
    /// points at, else `main`.
    pub fn main_branch(&self, configured: Option<&str>) -> Result<String, GitError> {
        if let Some(b) = configured {
            return Ok(b.to_string());
        }
        if let Some(s) = self.try_run(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])? {
            let s = s.trim();
            if let Some(b) = s.strip_prefix("origin/") {
                return Ok(b.to_string());
            }
        }
        Ok("main".to_string())
    }

    /// The ref to compare against for "merged": whichever of the local main
    /// branch and `origin/<main>` is further ahead (people working in
    /// worktrees rarely update local main). When they have diverged, the
    /// local branch. `None` if neither exists yet.
    pub fn main_ref(&self, main_branch: &str) -> Result<Option<String>, GitError> {
        let local = format!("refs/heads/{main_branch}");
        let remote = format!("refs/remotes/origin/{main_branch}");
        let l = self.rev_parse(&local)?;
        let r = self.rev_parse(&remote)?;
        Ok(match (l, r) {
            (Some(lc), Some(rc)) => {
                if lc != rc && self.is_ancestor(&lc, &rc)? {
                    Some(remote)
                } else {
                    Some(local)
                }
            }
            (Some(_), None) => Some(local),
            (None, Some(_)) => Some(remote),
            (None, None) => None,
        })
    }

    /// The merge base of two revisions, if they share history.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<String>, GitError> {
        Ok(self
            .try_run(&["merge-base", a, b])?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    /// Resolve `rev` to an object ID, or `None` if it doesn't exist.
    pub fn rev_parse(&self, rev: &str) -> Result<Option<String>, GitError> {
        Ok(self
            .try_run(&["rev-parse", "--verify", "--quiet", "--end-of-options", rev])?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    /// Every worktree of this clone, wherever it is on disk.
    pub fn worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        let text = self.run(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktrees(&text))
    }

    /// Every remote-tracking branch, excluding symbolic `HEAD` refs.
    pub fn remote_branches(&self) -> Result<Vec<BranchRef>, GitError> {
        self.refs("refs/remotes")
    }

    /// Every local branch.
    pub fn local_branches(&self) -> Result<Vec<BranchRef>, GitError> {
        self.refs("refs/heads")
    }

    fn refs(&self, namespace: &str) -> Result<Vec<BranchRef>, GitError> {
        let text = self.run(&[
            "for-each-ref",
            "--format=%(refname)%00%(refname:short)%00%(objectname)%00%(symref)",
            namespace,
        ])?;
        let mut out = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() < 4 || !parts[3].is_empty() || parts[0].ends_with("/HEAD") {
                continue;
            }
            out.push(BranchRef {
                refname: parts[0].to_string(),
                short: parts[1].to_string(),
                commit: parts[2].to_string(),
            });
        }
        Ok(out)
    }

    /// Local and remote-tracking branches whose tip is not reachable from
    /// `main_ref`: the ones that may hold work not on main yet.
    pub fn unmerged_branches(&self, main_ref: &str) -> Result<Vec<BranchRef>, GitError> {
        let no_merged = format!("--no-merged={main_ref}");
        let short = main_ref
            .strip_prefix("refs/heads/")
            .or_else(|| main_ref.strip_prefix("refs/remotes/origin/"))
            .unwrap_or(main_ref);
        let main_names = [
            format!("refs/heads/{short}"),
            format!("refs/remotes/origin/{short}"),
        ];
        let text = self.run(&[
            "for-each-ref",
            "--format=%(refname)%00%(refname:short)%00%(objectname)%00%(symref)",
            &no_merged,
            "refs/heads",
            "refs/remotes",
        ])?;
        let mut out = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() < 4 || !parts[3].is_empty() || parts[0].ends_with("/HEAD") {
                continue;
            }
            // The main branch itself, local or remote, is never "unmerged".
            if main_names.iter().any(|m| m == parts[0]) {
                continue;
            }
            out.push(BranchRef {
                refname: parts[0].to_string(),
                short: parts[1].to_string(),
                commit: parts[2].to_string(),
            });
        }
        Ok(out)
    }

    /// How many commits `head` is ahead of and behind `base`.
    pub fn ahead_behind(&self, base: &str, head: &str) -> Result<(u32, u32), GitError> {
        let spec = format!("{base}...{head}");
        let args = ["rev-list", "--left-right", "--count", spec.as_str()];
        let text = self.run(&args)?;
        let mut it = text.split_whitespace().map(|n| n.parse::<u32>());
        match (it.next(), it.next()) {
            (Some(Ok(behind)), Some(Ok(ahead))) => Ok((ahead, behind)),
            _ => Err(GitError::Parse {
                args: args.join(" "),
                detail: text.trim().to_string(),
            }),
        }
    }

    /// True if `commit` is reachable from `rev`.
    pub fn is_ancestor(&self, commit: &str, rev: &str) -> Result<bool, GitError> {
        let out = self
            .cmd()
            .args(["merge-base", "--is-ancestor", commit, rev])
            .output()
            .map_err(GitError::Spawn)?;
        Ok(out.status.success())
    }

    /// The tree ID of `dir` at `rev`, or `None` if `dir` doesn't exist there.
    /// Cheap, so callers can skip re-reading unchanged directories.
    pub fn tree_id(&self, rev: &str, dir: &str) -> Result<Option<String>, GitError> {
        self.rev_parse(&format!("{rev}:{}", dir.trim_end_matches('/')))
    }

    /// The files directly inside `dir` at `rev` (not recursive).
    pub fn list_dir(&self, rev: &str, dir: &str) -> Result<Vec<TreeFile>, GitError> {
        let spec = format!("{rev}:{}", dir.trim_end_matches('/'));
        let Some(text) = self.try_run(&["ls-tree", "-z", &spec])? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in text.split('\0').filter(|e| !e.is_empty()) {
            // "<mode> <type> <object>\t<name>"
            let Some((meta, name)) = entry.split_once('\t') else {
                continue;
            };
            let mut m = meta.split(' ');
            let (_, kind, blob) = (m.next(), m.next(), m.next());
            if kind == Some("blob")
                && let Some(blob) = blob
            {
                out.push(TreeFile {
                    name: name.to_string(),
                    blob: blob.to_string(),
                });
            }
        }
        Ok(out)
    }

    /// Read many blobs in one `git cat-file --batch` call. Missing or
    /// non-UTF-8 blobs are left out of the result.
    pub fn read_blobs(&self, ids: &[String]) -> Result<HashMap<String, String>, GitError> {
        let mut result = HashMap::new();
        if ids.is_empty() {
            return Ok(result);
        }
        let mut child = self
            .cmd()
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(GitError::Spawn)?;
        let input: String = ids.iter().map(|i| format!("{i}\n")).collect();
        let mut stdin = child.stdin.take().ok_or_else(|| GitError::Parse {
            args: "cat-file --batch".into(),
            detail: "no stdin".into(),
        })?;
        // Write from a thread so a large batch can't deadlock on full pipes.
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let stdout = child.stdout.take().ok_or_else(|| GitError::Parse {
            args: "cat-file --batch".into(),
            detail: "no stdout".into(),
        })?;
        let mut reader = BufReader::new(stdout);
        for id in ids {
            let mut header = String::new();
            if reader.read_line(&mut header).map_err(GitError::Spawn)? == 0 {
                break;
            }
            let parts: Vec<&str> = header.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == "missing" {
                continue;
            }
            let size: usize =
                parts
                    .get(2)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| GitError::Parse {
                        args: "cat-file --batch".into(),
                        detail: header.trim().to_string(),
                    })?;
            let mut buf = vec![0u8; size + 1]; // content plus trailing newline
            reader.read_exact(&mut buf).map_err(GitError::Spawn)?;
            buf.pop();
            if let Ok(text) = String::from_utf8(buf) {
                result.insert(id.clone(), text);
            }
        }
        let _ = writer.join();
        let _ = child.wait();
        Ok(result)
    }

    /// Read one file at `rev`, or `None` if it doesn't exist there.
    pub fn show_file(&self, rev: &str, path: &str) -> Result<Option<String>, GitError> {
        self.try_run(&["show", &format!("{rev}:{path}")])
    }

    /// Commits on `rev` made since `since` (an RFC 3339 time), newest first.
    pub fn commits_since(&self, rev: &str, since: &str) -> Result<Vec<String>, GitError> {
        let Some(text) = self.try_run(&["log", "--format=%H", &format!("--since={since}"), rev])?
        else {
            return Ok(Vec::new());
        };
        Ok(text.lines().map(str::to_string).collect())
    }
}

/// Parse `git worktree list --porcelain`.
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for (i, block) in text
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .enumerate()
    {
        let mut wt = Worktree {
            path: PathBuf::new(),
            head: None,
            branch: None,
            bare: false,
            is_main: i == 0,
        };
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                wt.path = PathBuf::from(p);
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                wt.head = Some(h.to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                wt.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line == "bare" {
                wt.bare = true;
            }
        }
        if !wt.path.as_os_str().is_empty() {
            out.push(wt);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(
            ok.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn parses_porcelain_worktrees() {
        let text =
            "worktree /a\nHEAD abc\nbranch refs/heads/main\n\nworktree /b\nHEAD def\ndetached\n\n";
        let wts = parse_worktrees(text);
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert!(wts[0].is_main);
        assert_eq!(wts[1].branch, None);
        assert!(!wts[1].is_main);
    }

    #[test]
    fn discovers_worktrees_and_reads_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join("docs/decisions")).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("docs/decisions/D-AAAA-x.md"), "hello\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "one"]);
        let other = tmp.path().join("elsewhere/feature");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                other.to_str().unwrap(),
            ],
        );
        std::fs::write(other.join("docs/decisions/D-BBBB-y.md"), "world\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "two"]);

        let repo = Repo::discover(&other).unwrap();
        let main_repo = Repo::discover(&main).unwrap();
        assert_eq!(repo.common_dir, main_repo.common_dir);
        assert_eq!(repo.current_branch().unwrap().as_deref(), Some("feature"));
        assert_eq!(repo.main_branch(None).unwrap(), "main");
        assert_eq!(
            repo.main_ref("main").unwrap().as_deref(),
            Some("refs/heads/main")
        );

        let wts = repo.worktrees().unwrap();
        assert_eq!(wts.len(), 2);
        assert!(wts.iter().any(|w| w.branch.as_deref() == Some("feature")));

        assert_eq!(repo.ahead_behind("main", "feature").unwrap(), (1, 0));
        let files = repo.list_dir("feature", "docs/decisions").unwrap();
        assert_eq!(files.len(), 2);
        let blobs = repo
            .read_blobs(&files.iter().map(|f| f.blob.clone()).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(blobs.len(), 2);
        assert!(blobs.values().any(|t| t == "world\n"));
        assert!(repo.tree_id("main", "docs/decisions").unwrap().is_some());
        assert!(repo.tree_id("main", "nope").unwrap().is_none());
        let head = repo.head_commit().unwrap().unwrap();
        assert!(!repo.is_ancestor(&head, "main").unwrap());
        assert_eq!(
            repo.show_file("main", "docs/decisions/D-AAAA-x.md")
                .unwrap()
                .as_deref(),
            Some("hello\n")
        );
    }
}
