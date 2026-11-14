//! Finding decisions everywhere they exist: the main branch, every worktree
//! (including uncommitted files) and every unmerged branch, local or remote
//! (D-8K5D), and resolving each ID to one result with provenance (D-E1S7).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use inloop_core::{Id, Provenance, State};
use inloop_git::{GitError, Repo};
use serde::Serialize;

use crate::model::{Decision, ParseError, id_from_file_name};

/// Where a copy of a decision file was found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    /// Committed on the main branch.
    Main,
    /// A file on disk in a worktree.
    Worktree {
        path: PathBuf,
        branch: Option<String>,
        /// The worktree InLoop is running in.
        current: bool,
        /// Not committed on the worktree's branch (new or changed).
        uncommitted: bool,
        /// Committed, and the branch never changed it since it left main:
        /// whatever main says now is what counts.
        #[serde(skip)]
        untouched: bool,
        #[serde(skip)]
        head: Option<String>,
    },
    /// Committed on a branch not checked out in any worktree.
    Branch { refname: String, short: String },
}

impl Place {
    pub fn describe(&self) -> String {
        match self {
            Place::Main => "main".into(),
            Place::Worktree {
                path,
                branch,
                uncommitted,
                ..
            } => format!(
                "{}{} ({})",
                branch.as_deref().unwrap_or("detached"),
                if *uncommitted { ", uncommitted" } else { "" },
                path.display()
            ),
            Place::Branch { short, .. } => format!("branch {short}"),
        }
    }
}

/// One copy of one decision file.
#[derive(Clone, Debug)]
pub struct Found {
    pub place: Place,
    pub file_name: String,
    pub text: String,
    pub parsed: Result<Decision, ParseError>,
}

/// A decision resolved to the copy that matters, with where it stands.
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub decision: Decision,
    pub file_name: String,
    /// The file on disk, when it's in a worktree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub provenance: Provenance,
    #[serde(skip)]
    pub place: Place,
}

/// Two different decisions using the same ID.
#[derive(Clone, Debug, Serialize)]
pub struct Collision {
    pub id: Id,
    pub copies: Vec<(String, String)>, // (file name, place)
}

/// A file that looks like a decision but doesn't parse.
#[derive(Clone, Debug, Serialize)]
pub struct Broken {
    pub file_name: String,
    pub place: String,
    pub error: String,
}

/// Every decision InLoop can see, resolved.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DecisionSet {
    pub entries: Vec<Entry>,
    pub collisions: Vec<Collision>,
    pub broken: Vec<Broken>,
    /// Every place each ID was seen, for lint.
    #[serde(skip)]
    pub found: Vec<Found>,
}

/// What to scan.
pub struct ScanOptions<'a> {
    pub decisions_dir: &'a str,
    pub main_branch: &'a str,
    /// Include other worktrees and unmerged branches (the default).
    pub include_unmerged: bool,
}

/// Scan the repository for decisions.
pub fn scan(repo: &Repo, opts: &ScanOptions<'_>) -> Result<DecisionSet, GitError> {
    let dir = opts.decisions_dir.trim_end_matches('/');
    let main_ref = repo.main_ref(opts.main_branch)?;
    let current_root = canonical(&repo.root);
    let mut found: Vec<Found> = Vec::new();
    let mut pending: Vec<(Place, String, String)> = Vec::new(); // place, file name, blob
    let mut seen_blobs: HashSet<String> = HashSet::new();

    // Main branch.
    let mut main_blobs: HashMap<String, String> = HashMap::new(); // file name -> blob
    if let Some(m) = &main_ref {
        for f in repo.list_dir(m, dir)? {
            if !f.name.ends_with(".md") {
                continue;
            }
            main_blobs.insert(f.name.clone(), f.blob.clone());
            seen_blobs.insert(f.blob.clone());
            pending.push((Place::Main, f.name, f.blob));
        }
    }

    // Worktrees, read from disk.
    let mut checked_out: HashSet<String> = HashSet::new();
    let mut disk: Vec<(Place, String, String)> = Vec::new(); // place, name, text
    let mut head_blob_lookups: Vec<(usize, String)> = Vec::new();
    // Disk entries whose HEAD blob equals the merge-base blob.
    let mut same_as_base: HashSet<usize> = HashSet::new();
    for wt in repo.worktrees()? {
        if wt.bare {
            continue;
        }
        let is_current = canonical(&wt.path) == current_root;
        if !is_current && !opts.include_unmerged {
            continue;
        }
        if let Some(b) = &wt.branch {
            checked_out.insert(b.clone());
        }
        let head_files: HashMap<String, String> = match &wt.head {
            Some(h) => repo
                .list_dir(h, dir)?
                .into_iter()
                .map(|f| (f.name, f.blob))
                .collect(),
            None => HashMap::new(),
        };
        let base_files: HashMap<String, String> = match (&main_ref, &wt.head) {
            (Some(m), Some(h)) => match repo.merge_base(m, h)? {
                Some(base) => repo
                    .list_dir(&base, dir)?
                    .into_iter()
                    .map(|f| (f.name, f.blob))
                    .collect(),
                None => HashMap::new(),
            },
            _ => HashMap::new(),
        };
        let Ok(rd) = std::fs::read_dir(wt.path.join(dir)) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".md"))
            .collect();
        names.sort();
        for name in names {
            let Ok(text) = std::fs::read_to_string(wt.path.join(dir).join(&name)) else {
                continue;
            };
            if let Some(blob) = head_files.get(&name) {
                if base_files.get(&name) == Some(blob) {
                    same_as_base.insert(disk.len());
                }
                head_blob_lookups.push((disk.len(), blob.clone()));
            }
            disk.push((
                Place::Worktree {
                    path: wt.path.clone(),
                    branch: wt.branch.clone(),
                    current: is_current,
                    uncommitted: true, // until shown to match HEAD
                    untouched: false,
                    head: wt.head.clone(),
                },
                name,
                text,
            ));
        }
    }

    // Unmerged branches not checked out anywhere.
    if opts.include_unmerged
        && let Some(m) = &main_ref
    {
        for b in repo.unmerged_branches(m)? {
            let local_name = b.refname.strip_prefix("refs/heads/");
            if local_name.is_some_and(|n| checked_out.contains(n)) {
                continue;
            }
            for f in repo.list_dir(&b.refname, dir)? {
                if !f.name.ends_with(".md") || !seen_blobs.insert(f.blob.clone()) {
                    continue;
                }
                pending.push((
                    Place::Branch {
                        refname: b.refname.clone(),
                        short: b.short.clone(),
                    },
                    f.name,
                    f.blob,
                ));
            }
        }
    }

    // Read every blob we need in one go.
    let mut want: Vec<String> = pending.iter().map(|p| p.2.clone()).collect();
    want.extend(head_blob_lookups.iter().map(|(_, b)| b.clone()));
    want.sort();
    want.dedup();
    let blobs = repo.read_blobs(&want)?;

    for (i, blob) in &head_blob_lookups {
        if let (Some(committed), Some(entry)) = (blobs.get(blob), disk.get_mut(*i))
            && *committed == entry.2
            && let Place::Worktree {
                uncommitted,
                untouched,
                ..
            } = &mut entry.0
        {
            *uncommitted = false;
            *untouched = same_as_base.contains(i);
        }
    }

    for (place, name, blob) in pending {
        if let Some(text) = blobs.get(&blob) {
            let parsed = Decision::parse(text);
            found.push(Found {
                place,
                file_name: name,
                text: text.clone(),
                parsed,
            });
        }
    }
    for (place, name, text) in disk {
        let parsed = Decision::parse(&text);
        found.push(Found {
            place,
            file_name: name,
            text,
            parsed,
        });
    }

    let main_texts: HashMap<String, String> = main_blobs
        .iter()
        .filter_map(|(n, b)| blobs.get(b).map(|t| (n.clone(), t.clone())))
        .collect();
    Ok(resolve(repo, main_ref.as_deref(), found, &main_texts, dir))
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn priority(place: &Place) -> u8 {
    match place {
        Place::Worktree { current: true, .. } => 0,
        Place::Main => 1,
        Place::Worktree { .. } => 2,
        Place::Branch { .. } => 3,
    }
}

fn resolve(
    repo: &Repo,
    main_ref: Option<&str>,
    found: Vec<Found>,
    main_texts: &HashMap<String, String>,
    dir: &str,
) -> DecisionSet {
    let mut set = DecisionSet::default();
    let mut by_id: BTreeMap<Id, Vec<usize>> = BTreeMap::new();
    for (i, f) in found.iter().enumerate() {
        match &f.parsed {
            Ok(d) => by_id.entry(d.id.clone()).or_default().push(i),
            Err(e) => {
                // Only report files that look like decisions.
                if id_from_file_name(&f.file_name).is_some() {
                    set.broken.push(Broken {
                        file_name: f.file_name.clone(),
                        place: f.place.describe(),
                        error: e.to_string(),
                    });
                }
            }
        }
    }
    let mut ahead_cache: HashMap<String, Option<u32>> = HashMap::new();
    for (id, idxs) in by_id {
        // Different file names under one ID means two different decisions.
        let names: HashSet<&str> = idxs.iter().map(|i| found[*i].file_name.as_str()).collect();
        if names.len() > 1 {
            let mut copies: Vec<(String, String)> = idxs
                .iter()
                .map(|i| (found[*i].file_name.clone(), found[*i].place.describe()))
                .collect();
            copies.sort();
            copies.dedup();
            set.collisions.push(Collision {
                id: id.clone(),
                copies,
            });
        }
        let Some(&first) = idxs.iter().min_by_key(|i| priority(&found[**i].place)) else {
            continue;
        };
        // A worktree copy its branch never changed defers to main's copy
        // when main has moved on (a worktree behind main).
        let mut best = first;
        if let Place::Worktree {
            untouched: true, ..
        } = &found[best].place
            && let Some(&m) = idxs.iter().find(|i| found[**i].place == Place::Main)
            && found[m].text != found[best].text
        {
            best = m;
        }
        let f = &found[best];
        let Ok(decision) = f.parsed.clone() else {
            continue;
        };
        let on_main_identical = main_texts.get(&f.file_name).is_some_and(|t| *t == f.text);
        let (provenance, path) = match &f.place {
            Place::Main => (Provenance::merged(), None),
            Place::Worktree {
                path,
                branch,
                current: _,
                uncommitted,
                untouched: _,
                head,
            } => {
                let file = path.join(dir).join(&f.file_name);
                if on_main_identical {
                    (Provenance::merged(), Some(file))
                } else {
                    let mut p = Provenance::unmerged(branch.clone(), Some(path.clone()));
                    p.uncommitted = *uncommitted;
                    if let (Some(m), Some(h)) = (main_ref, head) {
                        p.ahead_of_main = *ahead_cache
                            .entry(h.clone())
                            .or_insert_with(|| repo.ahead_behind(m, h).ok().map(|(a, _)| a));
                    }
                    (p, Some(file))
                }
            }
            Place::Branch { refname, short } => {
                let mut p = Provenance::unmerged(Some(short.clone()), None);
                if let Some(m) = main_ref {
                    p.ahead_of_main = *ahead_cache
                        .entry(refname.clone())
                        .or_insert_with(|| repo.ahead_behind(m, refname).ok().map(|(a, _)| a));
                }
                (p, None)
            }
        };
        set.entries.push(Entry {
            decision,
            file_name: f.file_name.clone(),
            path,
            provenance,
            place: f.place.clone(),
        });
    }
    set.found = found;
    set
}

impl DecisionSet {
    /// Look up a decision by ID or alias.
    pub fn get(&self, text: &str) -> Option<&Entry> {
        if let Ok(id) = Id::parse_loose(text)
            && let Some(e) = self.entries.iter().find(|e| e.decision.id == id)
        {
            return Some(e);
        }
        self.entries.iter().find(|e| e.decision.answers_to(text))
    }

    /// True if any copy anywhere uses this ID (or has it as an alias).
    pub fn id_taken(&self, id: &Id) -> bool {
        self.found.iter().any(|f| {
            id_from_file_name(&f.file_name).as_ref() == Some(id)
                || f.parsed.as_ref().is_ok_and(|d| d.answers_to(id.as_str()))
        })
    }

    pub fn merged(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| e.provenance.state == State::Merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
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
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn decision(id: &str, title: &str) -> String {
        format!(
            "---\nid: {id}\ntitle: \"{title}\"\nstatus: proposed\ndate: 2026-11-14\ndecided_by: agent\nbasis: null\nsession: null\nsupersedes: []\nsuperseded_by: null\naliases: []\ntags: []\n---\n\n## Decision\n\n{title}.\n"
        )
    }

    fn write(dir: &Path, id: &str, title: &str) {
        let d = Decision::parse(&decision(id, title)).unwrap();
        std::fs::write(dir.join("docs/decisions").join(d.file_name()), d.render()).unwrap();
    }

    #[test]
    fn sees_main_worktrees_and_branches_with_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join("docs/decisions")).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        write(&main, "D-AAAA", "On main");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "one"]);

        // A worktree somewhere else with a committed decision.
        let wt = tmp.path().join("far/away/feature");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                wt.to_str().unwrap(),
            ],
        );
        write(&wt, "D-BBBB", "On feature");
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "two"]);
        // And an uncommitted one.
        write(&wt, "D-CCCC", "Uncommitted");

        // A branch not checked out anywhere.
        git(&main, &["branch", "side", "main"]);
        let side = tmp.path().join("side");
        git(
            &main,
            &["worktree", "add", "-q", side.to_str().unwrap(), "side"],
        );
        write(&side, "D-DDDD", "On side");
        git(&side, &["add", "."]);
        git(&side, &["commit", "-q", "-m", "three"]);
        git(&main, &["worktree", "remove", side.to_str().unwrap()]);

        // A colliding ID on main's working copy.
        write(&main, "D-BBBB", "Different decision same id");

        let repo = Repo::discover(&main).unwrap();
        let set = scan(
            &repo,
            &ScanOptions {
                decisions_dir: "docs/decisions",
                main_branch: "main",
                include_unmerged: true,
            },
        )
        .unwrap();

        let a = set.get("D-AAAA").unwrap();
        assert_eq!(a.provenance.state, State::Merged);

        let c = set.get("d-cccc").unwrap();
        assert_eq!(c.provenance.state, State::Unmerged);
        assert!(c.provenance.uncommitted);
        assert_eq!(c.provenance.branch.as_deref(), Some("feature"));

        let d = set.get("D-DDDD").unwrap();
        assert_eq!(d.provenance.branch.as_deref(), Some("side"));
        assert_eq!(d.provenance.worktree, None);
        assert_eq!(d.provenance.ahead_of_main, Some(1));

        // The current worktree's copy wins for D-BBBB, and the clash is reported.
        let b = set.get("D-BBBB").unwrap();
        assert_eq!(b.decision.title, "Different decision same id");
        assert_eq!(set.collisions.len(), 1);
        assert!(set.id_taken(&Id::parse_loose("D-DDDD").unwrap()));
        assert!(!set.id_taken(&Id::parse_loose("D-ZZZZ").unwrap()));

        // Without unmerged sources, only main and this worktree.
        let only = scan(
            &repo,
            &ScanOptions {
                decisions_dir: "docs/decisions",
                main_branch: "main",
                include_unmerged: false,
            },
        )
        .unwrap();
        assert!(only.get("D-DDDD").is_none());
        assert!(only.get("D-CCCC").is_none());
    }

    #[test]
    fn a_worktree_behind_main_sees_mains_version() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join("docs/decisions")).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        write(&main, "D-AAAA", "Proposed then accepted");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "one"]);
        let wt = tmp.path().join("feat");
        git(
            &main,
            &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        );
        // Main accepts it after feat branched off.
        let f = main.join("docs/decisions/D-AAAA-proposed-then-accepted.md");
        let text = std::fs::read_to_string(&f)
            .unwrap()
            .replace("status: proposed", "status: rejected");
        std::fs::write(&f, text).unwrap();
        git(&main, &["commit", "-qam", "reject"]);

        let repo = Repo::discover(&wt).unwrap();
        let opts = ScanOptions {
            decisions_dir: "docs/decisions",
            main_branch: "main",
            include_unmerged: true,
        };
        let set = scan(&repo, &opts).unwrap();
        let e = set.get("D-AAAA").unwrap();
        assert_eq!(e.decision.status, crate::Status::Rejected);
        assert_eq!(e.provenance.state, State::Merged);

        // If feat changes it, feat's copy is its own unmerged work again.
        let wf = wt.join("docs/decisions/D-AAAA-proposed-then-accepted.md");
        let t = std::fs::read_to_string(&wf)
            .unwrap()
            .replace("tags: []", "tags: [feat]");
        std::fs::write(&wf, t).unwrap();
        let set = scan(&repo, &opts).unwrap();
        let e = set.get("D-AAAA").unwrap();
        assert_eq!(e.provenance.state, State::Unmerged);
        assert_eq!(e.decision.tags, vec!["feat"]);
    }

    #[test]
    fn origin_main_ahead_of_local_main_counts_as_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join("docs/decisions")).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        write(&main, "D-AAAA", "First");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "one"]);
        // Someone else's work lands on origin/main; local main is stale.
        git(&main, &["checkout", "-q", "-b", "tmp"]);
        write(&main, "D-BBBB", "From a co-founder");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "two"]);
        git(&main, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(&main, &["checkout", "-q", "main"]);
        git(&main, &["branch", "-q", "-D", "tmp"]);

        let repo = Repo::discover(&main).unwrap();
        assert_eq!(
            repo.main_ref("main").unwrap().as_deref(),
            Some("refs/remotes/origin/main")
        );
        let set = scan(
            &repo,
            &ScanOptions {
                decisions_dir: "docs/decisions",
                main_branch: "main",
                include_unmerged: true,
            },
        )
        .unwrap();
        let b = set.get("D-BBBB").unwrap();
        assert_eq!(b.provenance.state, State::Merged, "{:?}", b.provenance);
    }
}
