//! Behaviour of the public API, layer by layer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use inloop_redact::{PatternRule, RedactError, Redactor, RedactorConfig};

fn plain() -> Redactor {
    Redactor::new(&RedactorConfig {
        root: "/nonexistent/inloop-redact-test".into(),
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap()
}

fn with_env(env: &str) -> (tempfile::TempDir, Redactor) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), env).unwrap();
    let r = Redactor::new(&RedactorConfig {
        root: dir.path().to_path_buf(),
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap();
    (dir, r)
}

#[test]
fn known_values_match_every_encoding() {
    let value = "Wx9/fake+EXAMPLE secret:\"q\"";
    let (_dir, r) = with_env(&format!("ODD_THING_PASS='{value}'\n"));
    assert_eq!(r.known_value_count(), 1);
    let forms = [
        value.to_string(),
        STANDARD.encode(value),
        URL_SAFE_NO_PAD.encode(value),
        "Wx9%2Ffake%2BEXAMPLE%20secret%3A%22q%22".to_string(),
        "Wx9%2ffake%2bEXAMPLE%20secret%3a%22q%22".to_string(),
        {
            let json = serde_json::to_string(value).unwrap();
            json[1..json.len() - 1].to_string()
        },
    ];
    for form in forms {
        // No keyword nearby, so only layer 1 can catch these.
        let text = format!("blob {form} end");
        let out = r.redact_text(&text).unwrap();
        assert_eq!(out, "blob [REDACTED:known-value] end", "form {form:?}");
    }
}

#[test]
fn known_values_found_in_nested_env_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
    std::fs::write(
        dir.path().join("apps/web/.env.local"),
        "X_PASS=nested-EXAMPLE-value-0\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::write(
        dir.path().join("node_modules/pkg/.env"),
        "Y_PASS=ignored-EXAMPLE-value-0\n",
    )
    .unwrap();
    let r = Redactor::new(&RedactorConfig {
        root: dir.path().to_path_buf(),
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap();
    assert_eq!(r.known_value_count(), 1);
    assert_eq!(
        r.redact_text("a nested-EXAMPLE-value-0 b").unwrap(),
        "a [REDACTED:known-value] b"
    );
}

#[test]
fn missing_known_value_files_are_fine() {
    let r = plain();
    assert_eq!(r.known_value_count(), 0);
}

#[cfg(unix)]
#[test]
fn unreadable_known_value_file_fails_closed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "A_SECRET=EXAMPLE-0000-fake-value\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        // Running as root: permissions do not apply, nothing to test.
        return;
    }
    let result = Redactor::new(&RedactorConfig {
        root: dir.path().to_path_buf(),
        include_process_env: false,
        ..RedactorConfig::default()
    });
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    match result {
        Err(RedactError::Io { path: p, .. }) => assert!(p.ends_with(".env")),
        other => panic!("expected an Io error, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn bad_project_pattern_fails_closed() {
    let result = Redactor::new(&RedactorConfig {
        patterns: vec![PatternRule {
            kind: "customer".into(),
            regex: "ACME-[0-9".into(),
        }],
        include_process_env: false,
        ..RedactorConfig::default()
    });
    match result {
        Err(RedactError::BadPattern { kind, .. }) => assert_eq!(kind, "customer"),
        other => panic!("expected BadPattern, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn bad_glob_fails_closed() {
    let result = Redactor::new(&RedactorConfig {
        known_value_files: vec!["[unclosed".into()],
        include_process_env: false,
        ..RedactorConfig::default()
    });
    assert!(matches!(result, Err(RedactError::BadGlob { .. })));
}

#[test]
fn project_patterns_and_secret_group() {
    let r = Redactor::new(&RedactorConfig {
        patterns: vec![
            PatternRule {
                kind: "customer".into(),
                regex: r"\bACME-[0-9]{6}\b".into(),
            },
            PatternRule {
                kind: "ticket-ref".into(),
                regex: r"ref=(?P<secret>[a-z]{5})".into(),
            },
            PatternRule {
                kind: "redacted-word".into(),
                regex: "REDACTED".into(),
            },
        ],
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap();
    assert_eq!(
        r.redact_text("ACME-104233 ref=abcde").unwrap(),
        "[REDACTED:customer] ref=[REDACTED:ticket-ref]"
    );
    // A project rule can never eat an existing marker.
    assert_eq!(
        r.redact_text("[REDACTED:customer]").unwrap(),
        "[REDACTED:customer]"
    );
    assert_eq!(r.rule_stats().project_rules, 3);
}

#[test]
fn redaction_is_idempotent() {
    let r = plain();
    let text = "token=ghp_EXAMPLEfakeTOKEN0123abcdEXAMPLEfakeT password: \"Tr0ub4dor&3\" \
                Authorization: Bearer abc123EXAMPLEfake456def postgres://a:b0bEXAMPLE@h/db";
    let once = r.redact_text(text).unwrap();
    assert_ne!(once, text);
    assert_eq!(r.redact_text(&once).unwrap(), once);
    assert!(r.find(&once).is_empty());
}

#[test]
fn report_counts_kinds() {
    let r = plain();
    let (out, report) = r
        .redact_text_with_report("a AKIAFAKEEXAMPLE23456 b AKIAFAKEEXAMPLE65432 c")
        .unwrap();
    assert_eq!(
        out,
        "a [REDACTED:aws-access-token] b [REDACTED:aws-access-token] c"
    );
    assert_eq!(report.counts.get("aws-access-token"), Some(&2));
}

#[test]
fn find_offsets_are_bytes_and_sorted() {
    let r = plain();
    let text = "é→ ghp_EXAMPLEfakeTOKEN0123abcdEXAMPLEfakeT and AKIAFAKEEXAMPLE23456";
    let found = r.find(text);
    assert_eq!(found.len(), 2);
    assert_eq!(
        &text[found[0].start..found[0].end],
        "ghp_EXAMPLEfakeTOKEN0123abcdEXAMPLEfakeT"
    );
    assert!(found[0].end <= found[1].start);
}

#[test]
fn the_gate_redacts_structured_values() {
    let r = plain();
    let key = "AKIAFAKEEXAMPLE23456";
    let json = serde_json::json!({
        "cmd": format!("aws --key {key}"),
        key: "as a key",
        "AKIAFAKEEXAMPLE65432": "another key",
        "nested": [{"n": 1, "s": key}, null, true],
    });
    let out = r.redact(json).unwrap().into_inner();
    let s = out.to_string();
    assert!(!s.contains("AKIAFAKE"), "{s}");
    assert_eq!(out["nested"][0]["n"], 1);
    // Two keys redacting to the same text keep both values.
    let obj = out.as_object().unwrap();
    assert_eq!(obj.len(), 4);
    assert!(obj.contains_key("[REDACTED:aws-access-token]#2"));

    let mut map = BTreeMap::new();
    map.insert("field".to_string(), Some(vec![format!("x {key}")]));
    let safe = r.redact(map).unwrap();
    assert_eq!(
        safe.get()["field"],
        Some(vec!["x [REDACTED:aws-access-token]".to_string()])
    );
    let serialised = serde_json::to_string(&safe).unwrap();
    assert!(serialised.contains("[REDACTED:aws-access-token]"));
}

#[test]
fn debug_never_prints_known_values() {
    let (_dir, r) = with_env("A_TOKEN=EXAMPLE-very-secret-value-0001\n");
    let dbg = format!("{r:?}");
    assert!(!dbg.contains("EXAMPLE-very-secret"), "{dbg}");
    assert!(dbg.contains("known_values: 1"), "{dbg}");
}

#[test]
fn rule_stats_are_sane() {
    let stats = plain().rule_stats();
    assert!(stats.provider_rules > 200, "{stats:?}");
    assert!(stats.skipped_rules <= 4, "{stats:?}");
    assert_eq!(stats.project_rules, 0);
    let with_pd = Redactor::new(&RedactorConfig {
        personal_data: true,
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap();
    assert_eq!(with_pd.rule_stats().project_rules, 3);
}

#[test]
fn utf8_text_around_matches_is_preserved() {
    let r = plain();
    let text = "日本語 ghp_EXAMPLEfakeTOKEN0123abcdEXAMPLEfakeT ✓ done";
    assert_eq!(
        r.redact_text(text).unwrap(),
        "日本語 [REDACTED:github-pat] ✓ done"
    );
}
