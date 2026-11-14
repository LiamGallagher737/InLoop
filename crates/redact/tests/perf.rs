//! Performance: redacting a 5 MB realistic transcript must take under a second
//! in release mode. Ignored by default because debug builds are far slower:
//!
//! ```sh
//! cargo test --release -- --ignored perf
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Instant;

use inloop_redact::{Redactor, RedactorConfig};

/// About 5 MB built from the corpus fixtures (transcripts, shell output,
/// code, configs, logs), with secrets in them, repeated with a varying line
/// so the text is not one identical block.
fn transcript() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut parts = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        for f in std::fs::read_dir(&path).unwrap() {
            let f = f.unwrap().path();
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            if name.starts_with("input") {
                parts.push(std::fs::read_to_string(&f).unwrap());
            }
        }
    }
    let mut out = String::with_capacity(5_300_000);
    let mut i = 0usize;
    while out.len() < 5 * 1024 * 1024 {
        out.push_str(&parts[i % parts.len()]);
        out.push_str(&format!(
            "{{\"type\":\"assistant\",\"uuid\":\"{i:08x}-0000-4000-8000-000000000000\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Step {i}: ran the tests, all green.\"}}]}}}}\n"
        ));
        i += 1;
    }
    out
}

#[test]
#[ignore]
fn perf_redact_5mb_transcript() {
    let text = transcript();
    let r = Redactor::new(&RedactorConfig {
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap();
    // Warm up the lazy DFAs once, as a long-running process would be.
    let _ = r.redact_text(&text[..100_000.min(text.len())]).unwrap();
    let start = Instant::now();
    let (out, report) = r.redact_text_with_report(&text).unwrap();
    let elapsed = start.elapsed();
    let total: usize = report.counts.values().sum();
    eprintln!(
        "redacted {:.2} MB in {:?} ({} matches, output {:.2} MB)",
        text.len() as f64 / 1_048_576.0,
        elapsed,
        total,
        out.len() as f64 / 1_048_576.0
    );
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
#[ignore]
fn perf_construct_redactor() {
    let start = Instant::now();
    let r = Redactor::new(&RedactorConfig::default()).unwrap();
    eprintln!(
        "Redactor::new took {:?} ({:?})",
        start.elapsed(),
        r.rule_stats()
    );
}
