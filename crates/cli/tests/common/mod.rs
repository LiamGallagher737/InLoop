//! Shared helpers for the end-to-end tests.
#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test Person")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Test Person")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub struct Repo {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
}

impl Repo {
    pub fn new() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test Person"]);
        git(&root, &["config", "user.email", "t@example.com"]);
        let r = Repo {
            _tmp: tmp,
            root,
            home,
        };
        let out = r.run(&["init", "--no-hooks"]);
        assert!(out.status.success(), "{}", stderr(&out));
        git(&r.root, &["add", "."]);
        git(&r.root, &["commit", "-q", "-m", "init"]);
        r
    }

    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_inloop"))
            .args(args)
            .current_dir(dir)
            .env("HOME", &self.home)
            .env("INLOOP_CONFIG_DIR", self.home.join("config"))
            .env("INLOOP_DATA_DIR", self.home.join("data"))
            .env("NO_COLOR", "1")
            .env_remove("INLOOP_SESSION_ID")
            .output()
            .unwrap()
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.root, args)
    }

    pub fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut a = vec!["--json"];
        a.extend_from_slice(args);
        let out = self.run(&a);
        assert!(out.status.success(), "inloop {args:?}: {}", stderr(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    pub fn git(&self, args: &[&str]) {
        git(&self.root, args)
    }
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
