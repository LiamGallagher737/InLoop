//! Where a result stands (D-E1S7): merged on the main branch, or only on
//! another branch or worktree.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Whether a result is settled on the main branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Committed on the main branch.
    Merged,
    /// Only on another branch or worktree, or not committed yet. Work in
    /// progress, not settled fact.
    Unmerged,
}

/// The provenance record every decision and session result carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub state: State,
    /// The branch the result was found on, when it isn't merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The worktree where that branch is checked out, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<PathBuf>,
    /// Commits the branch is ahead of the main branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead_of_main: Option<u32>,
    /// True when the result exists only as an uncommitted file in a worktree.
    #[serde(default, skip_serializing_if = "is_false")]
    pub uncommitted: bool,
    /// The PM task that owns the branch. Only set when the PM layer is in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Provenance {
    pub fn merged() -> Self {
        Self {
            state: State::Merged,
            branch: None,
            worktree: None,
            ahead_of_main: None,
            uncommitted: false,
            task: None,
        }
    }

    pub fn unmerged(branch: Option<String>, worktree: Option<PathBuf>) -> Self {
        Self {
            state: State::Unmerged,
            branch,
            worktree,
            ahead_of_main: None,
            uncommitted: false,
            task: None,
        }
    }

    pub fn is_merged(&self) -> bool {
        self.state == State::Merged
    }

    /// A one-line description for people, empty when merged. For example
    /// `NOT ON MAIN · branch snap-writes · ~/code/x/snap-writes (3 ahead)`.
    pub fn status_line(&self, home: Option<&std::path::Path>) -> String {
        if self.is_merged() {
            return String::new();
        }
        let mut parts = vec![if self.uncommitted {
            "NOT COMMITTED".to_string()
        } else {
            "NOT ON MAIN".to_string()
        }];
        if let Some(b) = &self.branch {
            parts.push(format!("branch {b}"));
        }
        if let Some(w) = &self.worktree {
            let shown = match home.and_then(|h| w.strip_prefix(h).ok()) {
                Some(rest) => format!("~/{}", rest.display()),
                None => w.display().to_string(),
            };
            match self.ahead_of_main {
                Some(n) if n > 0 => parts.push(format!("{shown} ({n} ahead)")),
                _ => parts.push(shown),
            }
        } else if let Some(n) = self.ahead_of_main.filter(|n| *n > 0) {
            parts.push(format!("{n} ahead"));
        }
        if let Some(t) = &self.task {
            parts.push(format!("task {t}"));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_serialises_to_just_the_state() {
        let json = serde_json::to_string(&Provenance::merged()).unwrap_or_default();
        assert_eq!(json, r#"{"state":"merged"}"#);
    }

    #[test]
    fn status_line_shows_where_it_is() {
        let mut p = Provenance::unmerged(
            Some("snap-writes".into()),
            Some("/Users/x/code/a/snap-writes".into()),
        );
        p.ahead_of_main = Some(3);
        let line = p.status_line(Some(std::path::Path::new("/Users/x")));
        assert_eq!(
            line,
            "NOT ON MAIN · branch snap-writes · ~/code/a/snap-writes (3 ahead)"
        );
        assert_eq!(Provenance::merged().status_line(None), "");
    }
}
