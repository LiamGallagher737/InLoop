//! Runs every fixture in `tests/corpus/` through the redactor.
//!
//! Each fixture directory has an input file and an `expected.toml` manifest:
//! `secrets` must be gone from the output (whole, and for long secrets, any
//! 20-character window of them), `survive` must appear exactly as often as in
//! the input. The fixture directory is the redactor root, so a fixture can
//! carry its own `.env` of known values. The test prints caught and missed
//! counts per fixture and fails on any miss or any damaged look-alike.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use inloop_redact::{PatternRule, Redactor, RedactorConfig};
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    description: String,
    input: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    dropped: Option<bool>,
    #[serde(default)]
    personal_data: bool,
    #[serde(default)]
    patterns: Vec<PatternRule>,
    #[serde(default)]
    secrets: Vec<String>,
    #[serde(default)]
    survive: Vec<String>,
}

const WINDOW: usize = 20;

fn leaked(output: &str, secret: &str) -> bool {
    if output.contains(secret) {
        return true;
    }
    if secret.len() < 24 || !secret.is_ascii() {
        return false;
    }
    (0..=secret.len() - WINDOW).any(|i| output.contains(&secret[i..i + WINDOW]))
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("expected.toml").exists())
        .collect();
    out.sort();
    out
}

fn redactor_for(dir: &Path, m: &Manifest) -> Redactor {
    Redactor::new(&RedactorConfig {
        root: dir.to_path_buf(),
        patterns: m.patterns.clone(),
        personal_data: m.personal_data,
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap()
}

#[test]
fn corpus() {
    let mut failures = Vec::new();
    let (mut total_caught, mut total_secrets, mut total_survived, mut total_survive) = (0, 0, 0, 0);
    let fixtures = fixtures();
    assert!(fixtures.len() >= 10, "corpus is missing");
    eprintln!(
        "{:<22} {:>8} {:>8} {:>10}  description",
        "fixture", "caught", "missed", "survived"
    );
    for dir in fixtures {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(dir.join("expected.toml")).unwrap()).unwrap();
        let input = std::fs::read_to_string(dir.join(&manifest.input)).unwrap();
        let r = redactor_for(&dir, &manifest);
        let output = match &manifest.command {
            Some(cmd) => {
                let out = r.redact_command_output(cmd, &input).unwrap();
                let dropped = out == "[REDACTED:command-output]";
                if Some(dropped) != manifest.dropped {
                    failures.push(format!("{name}: command {cmd:?} dropped={dropped}"));
                }
                out
            }
            None => r.redact_text(&input).unwrap(),
        };

        let missed: Vec<&String> = manifest
            .secrets
            .iter()
            .filter(|s| leaked(&output, s))
            .collect();
        let damaged: Vec<&String> = manifest
            .survive
            .iter()
            .filter(|s| output.matches(s.as_str()).count() != input.matches(s.as_str()).count())
            .collect();
        let caught = manifest.secrets.len() - missed.len();
        let survived = manifest.survive.len() - damaged.len();
        eprintln!(
            "{name:<22} {caught:>8} {:>8} {:>5}/{:<4}  {}",
            missed.len(),
            survived,
            manifest.survive.len(),
            manifest.description
        );
        total_caught += caught;
        total_secrets += manifest.secrets.len();
        total_survived += survived;
        total_survive += manifest.survive.len();
        for s in missed {
            failures.push(format!("{name}: MISSED secret {s:?}"));
        }
        for s in damaged {
            failures.push(format!("{name}: DAMAGED look-alike {s:?}"));
        }

        // Idempotence over real-shaped text.
        let again = r.redact_text(&output).unwrap();
        if again != output {
            failures.push(format!("{name}: redaction is not idempotent"));
        }
        // `find` agrees with the report.
        if manifest.command.is_none() {
            let (_, report) = r.redact_text_with_report(&input).unwrap();
            let total: usize = report.counts.values().sum();
            if total != r.find(&input).len() {
                failures.push(format!("{name}: find and report disagree"));
            }
        }
    }
    eprintln!(
        "TOTAL: caught {total_caught}/{total_secrets} secrets, {total_survived}/{total_survive} look-alikes intact"
    );
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("{f}");
        }
        panic!("{} corpus failures", failures.len());
    }
}

/// Prints redacted output for one fixture: `CORPUS_SHOW=git-log cargo test show -- --nocapture`.
#[test]
fn show() {
    let Ok(want) = std::env::var("CORPUS_SHOW") else {
        return;
    };
    for dir in fixtures() {
        if dir.file_name().unwrap().to_string_lossy() != want {
            continue;
        }
        let manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(dir.join("expected.toml")).unwrap()).unwrap();
        let input = std::fs::read_to_string(dir.join(&manifest.input)).unwrap();
        let r = redactor_for(&dir, &manifest);
        println!("{}", r.redact_text(&input).unwrap());
        for f in r.find(&input) {
            println!("{} {:?}", f.kind, &input[f.start..f.end]);
        }
    }
}
