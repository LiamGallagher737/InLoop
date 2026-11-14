//! Layer 4: which commands have their whole output dropped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use inloop_redact::{Redactor, RedactorConfig};

fn redactor() -> Redactor {
    Redactor::new(&RedactorConfig {
        root: "/nonexistent/inloop-redact-test".into(),
        known_value_files: vec![
            ".env".into(),
            ".env.*".into(),
            "config/secrets/*.json".into(),
        ],
        include_process_env: false,
        ..RedactorConfig::default()
    })
    .unwrap()
}

const DUMPING: &[&str] = &[
    "env",
    "env | sort",
    "/usr/bin/env",
    "sudo env",
    "sudo -u deploy -E env | grep AWS",
    "FOO=1 BAR=2 env",
    "env -i",
    "printenv",
    "printenv OPENAI_API_KEY",
    "set",
    "export",
    "export -p",
    "declare -px",
    "cat .env",
    "cat ./.env.local",
    "less apps/web/.env.production",
    "head -n 5 .env",
    "tail -f ~/.netrc",
    "bat --plain .env.development",
    "grep -i stripe .env",
    "cat config/secrets/prod.json",
    "cat ~/.aws/credentials",
    "cat ~/.ssh/id_ed25519",
    "cat deploy.pem",
    "cat ~/.docker/config.json",
    "cat ~/.kube/config",
    "cat < .env",
    "cd app && cat .env",
    "ls; cat .env",
    "echo $(cat .env)",
    "echo \"$(cat .env)\"",
    "echo $OPENAI_API_KEY",
    "echo \"${GITHUB_TOKEN}\"",
    "printf '%s' \"$DB_PASSWORD\"",
    "security find-generic-password -s example -w",
    "security find-internet-password -gs github.com",
    "/usr/bin/security find-generic-password -a me -s svc -g",
    "security dump-keychain",
    "gcloud auth print-access-token",
    "gcloud auth application-default print-access-token",
    "gcloud secrets versions access latest --secret=api-key",
    "aws configure export-credentials --profile prod",
    "aws configure get aws_secret_access_key",
    "aws sts get-session-token",
    "aws ecr get-login-password --region us-east-1",
    "aws secretsmanager get-secret-value --secret-id prod/db",
    "aws ssm get-parameter --name /app/key --with-decryption",
    "op read op://vault/item/password",
    "op item get Stripe --reveal",
    "vault kv get secret/app",
    "vault read database/creds/app",
    "kubectl get secret app-secrets -o yaml",
    "kubectl get secrets -n prod -ojson",
    "kubectl get secret/app-secrets --output=jsonpath='{.data.password}'",
    "kubectl config view --raw",
    "heroku config -a example-app",
    "heroku config:get DATABASE_URL",
    "heroku auth:token",
    "gh auth token",
    "gh auth status --show-token",
    "docker inspect api",
    "docker exec -it api env",
    "docker exec api printenv DATABASE_URL",
    "docker compose config",
    "docker-compose config",
    "kubectl exec deploy/api -- env",
    "ssh prod 'cat /srv/app/.env'",
    "ssh prod printenv",
    "doppler run -- printenv",
    "doppler secrets",
    "aws-vault exec prod -- env",
    "bash -c 'env | sort'",
    "sh -c \"cat .env\"",
    "sops -d secrets.enc.yaml",
    "pass show work/aws",
    "az account get-access-token",
    "terraform output -json",
    "supabase status",
    "git show HEAD:.env",
    "git diff -- .env.production",
    "git credential fill",
    "cat /proc/1/environ",
    "openssl rsa -in server.key -text",
];

const SAFE: &[&str] = &[
    "ls -la",
    "cat README.md",
    "cat .env.example.md.txt.bak/notes.md",
    "grep -r TODO src/",
    "cargo test --workspace",
    "git status",
    "git log --oneline -5",
    "git diff src/lib.rs",
    "echo hello",
    "echo $HOME $PATH",
    "printf '%s\\n' done",
    "env FOO=1 cargo build",
    "env -u DEBUG npm test",
    "set -euo pipefail",
    "export FOO=bar",
    "kubectl get pods -o wide",
    "kubectl get secrets",
    "kubectl describe secret app-secrets",
    "docker ps",
    "docker run --rm -e FOO=1 alpine echo hi",
    "heroku config:set FOO=bar",
    "gh pr list",
    "gh auth status",
    "security find-generic-password -s example",
    "aws s3 ls",
    "gcloud config list",
    "vault status",
    "op item list",
    "cat ~/.ssh/id_ed25519.pub",
    "source .venv/bin/activate",
    "npm run build 2>&1 | tail -20",
    "curl -s https://api.example.com/health",
    "",
];

#[test]
fn dumping_commands_are_dropped() {
    let r = redactor();
    let missed: Vec<&&str> = DUMPING
        .iter()
        .filter(|c| !r.is_secret_dumping_command(c))
        .collect();
    assert!(missed.is_empty(), "not recognised as dumping: {missed:#?}");
}

#[test]
fn ordinary_commands_are_kept() {
    let r = redactor();
    let wrong: Vec<&&str> = SAFE
        .iter()
        .filter(|c| r.is_secret_dumping_command(c))
        .collect();
    assert!(wrong.is_empty(), "wrongly treated as dumping: {wrong:#?}");
}

#[test]
fn command_output_is_dropped_or_redacted() {
    let r = redactor();
    assert_eq!(
        r.redact_command_output("env", "HOME=/Users/x\nX=not-a-pattern")
            .unwrap(),
        "[REDACTED:command-output]"
    );
    assert_eq!(
        r.redact_command_output("ls", "key AKIAFAKEEXAMPLE23456")
            .unwrap(),
        "key [REDACTED:aws-access-token]"
    );
}
