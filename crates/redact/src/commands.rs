//! Layer 4: whole outputs.
//!
//! Some commands exist to print secrets: `env`, `printenv`, `cat .env`,
//! `security find-generic-password -w`, `gcloud auth print-access-token` and
//! so on. Their output is dropped entirely rather than cleaned, because a
//! cleaned dump of every secret is only as good as the patterns.

use globset::GlobSet;

use crate::RedactError;
use crate::entropy::name_looks_secret;
use crate::known::build_globsets;

/// Files that hold secrets whatever the project config says.
const SECRET_FILES: &[&str] = &[
    ".env",
    ".env.*",
    "*.env",
    ".envrc",
    ".netrc",
    "_netrc",
    ".pgpass",
    ".git-credentials",
    ".npmrc",
    ".pypirc",
    ".dockercfg",
    "credentials",
    "credentials.json",
    "credentials.toml",
    "service-account*.json",
    "*serviceaccount*.json",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_ecdsa_sk",
    "id_ed25519_sk",
    "*.tfvars",
    "*.tfstate",
    "*.tfstate.backup",
    "secrets.yml",
    "secrets.yaml",
    "secrets.json",
    "secrets.toml",
    ".secrets",
    "environ",
    ".docker/config.json",
    ".kube/config",
    "kubeconfig",
    "gh/hosts.yml",
    "master.key",
    "*.age",
];

/// Programs that print the files named in their arguments.
const FILE_READERS: &[&str] = &[
    "cat",
    "less",
    "more",
    "head",
    "tail",
    "bat",
    "batcat",
    "nl",
    "tac",
    "strings",
    "xxd",
    "od",
    "hexdump",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "ack",
    "awk",
    "gawk",
    "sed",
    "cut",
    "sort",
    "uniq",
    "column",
    "jq",
    "yq",
    "base64",
    "diff",
    "colordiff",
    "vim",
    "vi",
    "view",
    "nano",
    "emacs",
    "code",
    "open",
    "pbcopy",
    "type",
    "get-content",
    "gc",
    "openssl",
    "ssh-keygen",
    "dotenv",
    "source",
    "tee",
    "paste",
    "fold",
    "fmt",
    "pr",
    "rev",
    "zcat",
    "gzcat",
    "xargs",
];

/// Programs that run another command given in their arguments.
const WRAPPERS: &[&str] = &[
    "docker",
    "podman",
    "kubectl",
    "oc",
    "ssh",
    "fly",
    "flyctl",
    "heroku",
    "doppler",
    "op",
    "dotenvx",
    "dotenv",
    "aws-vault",
    "direnv",
    "uv",
    "poetry",
    "pipenv",
    "bundle",
    "npx",
    "pnpm",
    "yarn",
    "nix-shell",
    "railway",
    "infisical",
    "vercel",
    "devbox",
    "mise",
    "asdf",
    "chamber",
    "summon",
    "with-contenv",
    "su",
    "runuser",
    "watch",
    "timeout",
    "script",
];

/// Prefix words that are not the command itself.
const PREFIXES: &[&str] = &[
    "sudo",
    "doas",
    "command",
    "builtin",
    "exec",
    "time",
    "nohup",
    "nice",
    "ionice",
    "stdbuf",
    "caffeinate",
    "noglob",
    "then",
    "do",
    "else",
    "!",
    "{",
];

const MAX_DEPTH: usize = 4;

pub(crate) struct CommandPolicy {
    by_name: GlobSet,
    by_path: GlobSet,
}

impl CommandPolicy {
    pub(crate) fn new(known_value_files: &[String]) -> Result<CommandPolicy, RedactError> {
        let mut globs: Vec<String> = SECRET_FILES.iter().map(|s| s.to_string()).collect();
        globs.extend(known_value_files.iter().cloned());
        let (by_name, by_path) = build_globsets(&globs)?;
        Ok(CommandPolicy { by_name, by_path })
    }

    pub(crate) fn is_secret_dumping(&self, command: &str) -> bool {
        self.check(command, 0)
    }

    fn check(&self, command: &str, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return true;
        }
        segments(command)
            .iter()
            .any(|seg| self.segment_dumps(seg, depth))
    }

    fn segment_dumps(&self, tokens: &[String], depth: usize) -> bool {
        // Command substitution inside a quoted word: `echo "$(cat .env)"`.
        if depth < MAX_DEPTH
            && tokens
                .iter()
                .any(|t| (t.contains("$(") || t.contains('`')) && self.check(t, depth + 1))
        {
            return true;
        }
        let tokens = strip_prefixes(tokens);
        let Some(first) = tokens.first() else {
            return false;
        };
        let bin = basename(first).to_ascii_lowercase();
        let args = &tokens[1..];
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        let has = |w: &str| words.contains(&w);
        let has_any = |ws: &[&str]| ws.iter().any(|w| words.contains(w));
        let positional: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| !w.starts_with('-'))
            .collect();

        let dumps = match bin.as_str() {
            "env" => return self.env_dumps(args, depth),
            "printenv" => true,
            "set" => args.is_empty(),
            "export" => args.is_empty() || has("-p"),
            "declare" | "typeset" => {
                args.is_empty() || (positional.is_empty() && has_any(&["-p", "-x", "-px", "-xp"]))
            }
            "echo" | "printf" | "print" => args.iter().any(|a| mentions_secret_var(a)),
            "security" => {
                has("dump-keychain")
                    || (has_any(&["find-generic-password", "find-internet-password"])
                        && words.iter().any(|w| {
                            w.starts_with('-')
                                && !w.starts_with("--")
                                && (w.contains('w') || w.contains('g'))
                        }))
            }
            "gcloud" => {
                has_any(&[
                    "print-access-token",
                    "print-identity-token",
                    "config-helper",
                ]) || (has("secrets") && has("access"))
            }
            "aws" => {
                (has("configure") && has("export-credentials"))
                    || (has("configure")
                        && has("get")
                        && words.iter().any(|w| {
                            let l = w.to_ascii_lowercase();
                            l.contains("secret") || l.contains("token") || l.contains("key")
                        }))
                    || has_any(&[
                        "get-session-token",
                        "assume-role",
                        "assume-role-with-web-identity",
                        "assume-role-with-saml",
                        "get-federation-token",
                        "get-login-password",
                        "get-secret-value",
                        "create-access-key",
                        "get-role-credentials",
                        "get-authorization-token",
                    ])
                    || (has("ssm") && has("--with-decryption"))
            }
            "op" => {
                has_any(&["read", "inject"])
                    || (has("item") && has("get") && has_any(&["--reveal", "--fields"]))
                    || (has("signin") && has("--raw"))
            }
            "vault" => {
                (has("kv") && has("get"))
                    || positional.first() == Some(&"read")
                    || (has("token") && has("create"))
                    || has("login")
            }
            "kubectl" | "oc" => {
                let secret = words.iter().any(|w| {
                    matches!(*w, "secret" | "secrets")
                        || w.starts_with("secret/")
                        || w.starts_with("secrets/")
                });
                (has("get") && secret && output_shows_data(&words))
                    || (has("config") && has("view") && has_any(&["--raw", "--flatten"]))
                    || (has("create") && has("token"))
                    || self.wrapped_dumps(args, depth)
            }
            "heroku" => {
                positional.first().is_some_and(|w| {
                    *w == "config" || w.starts_with("config:get") || *w == "auth:token"
                }) || self.wrapped_dumps(args, depth)
            }
            "gh" => {
                (has("auth") && has("token"))
                    || (has("auth") && has("status") && has_any(&["-t", "--show-token"]))
            }
            "docker" | "podman" | "nerdctl" => {
                positional.first() == Some(&"inspect")
                    || (has("compose") && has("config"))
                    || self.wrapped_dumps(args, depth)
            }
            "docker-compose" => has("config") || self.wrapped_dumps(args, depth),
            "git" => {
                (has("credential") && has_any(&["fill", "get"]))
                    || (has_any(&["show", "diff", "log", "blame", "cat-file"])
                        && words.iter().any(|w| {
                            let path = w.rsplit_once(':').map_or(*w, |(_, p)| p);
                            !w.starts_with('-') && self.is_secret_file(path)
                        }))
            }
            "npm" | "pnpm" | "yarn" => {
                (has("token") && has("create")) || self.wrapped_dumps(args, depth)
            }
            "doppler" => {
                (has("secrets") && !has_any(&["set", "delete", "upload"]))
                    || (has("configs") && has("tokens") && has("create"))
                    || self.wrapped_dumps(args, depth)
            }
            "sops" => has_any(&["-d", "--decrypt", "decrypt"]),
            "ansible-vault" => has_any(&["view", "decrypt"]),
            "pass" | "gopass" => {
                has("show") || (positional.len() == 1 && !matches!(positional[0], "ls" | "list"))
            }
            "bw" => has("get"),
            "lpass" => has("show"),
            "az" => has("get-access-token") || (has("keyvault") && has("secret") && has("show")),
            "firebase" => words.iter().any(|w| w.starts_with("functions:config:get")),
            "railway" => has("variables") || self.wrapped_dumps(args, depth),
            "supabase" => has("status"),
            "infisical" => has_any(&["secrets", "export"]) || self.wrapped_dumps(args, depth),
            "terraform" | "tofu" => {
                (has("output") && has_any(&["-json", "-raw"])) || (has("show") && has("-json"))
            }
            "bash" | "sh" | "zsh" | "fish" | "dash" | "ksh" => {
                let mut dumps = false;
                for (i, w) in words.iter().enumerate() {
                    if w.starts_with('-') && w.contains('c') && !w.starts_with("--") {
                        if let Some(script) = words.get(i + 1) {
                            dumps = self.check(script, depth + 1);
                        }
                        break;
                    }
                }
                dumps || words.iter().any(|w| self.is_secret_file(w))
            }
            "eval" => self.check(&words.join(" "), depth + 1),
            _ if WRAPPERS.contains(&bin.as_str()) => self.wrapped_dumps(args, depth),
            _ => false,
        };
        if dumps {
            return true;
        }
        if FILE_READERS.contains(&bin.as_str()) || bin == "." {
            return words.iter().any(|w| {
                let w = w.trim_start_matches('<');
                !w.starts_with('-') && self.is_secret_file(w)
            });
        }
        false
    }

    /// `env` alone dumps; `env FOO=1 cmd` runs `cmd`.
    fn env_dumps(&self, args: &[String], depth: usize) -> bool {
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            if a == "-u" || a == "--unset" || a == "-C" || a == "--chdir" || a == "-S" {
                i += 2;
            } else if a.starts_with('-') || is_assignment(a) {
                i += 1;
            } else {
                break;
            }
        }
        if i >= args.len() {
            return true;
        }
        self.segment_dumps(&args[i..], depth + 1)
    }

    /// Does any command inside a wrapper (`docker exec c env`, `ssh host
    /// printenv`, `doppler run -- env`) dump?
    fn wrapped_dumps(&self, args: &[String], depth: usize) -> bool {
        if depth >= MAX_DEPTH {
            return false;
        }
        if let Some(pos) = args.iter().position(|a| a == "--") {
            return self.segment_dumps(&args[pos + 1..], depth + 1);
        }
        (0..args.len()).any(|i| {
            let a = &args[i];
            if a.starts_with('-') || is_assignment(a) {
                return false;
            }
            // A quoted remote command: `ssh host 'cat .env'`.
            if a.contains(' ') && self.check(a, depth + 1) {
                return true;
            }
            let bin = basename(a).to_ascii_lowercase();
            matches!(
                bin.as_str(),
                "env" | "printenv" | "set" | "export" | "cat" | "bash" | "sh"
            ) && self.segment_dumps(&args[i..], depth + 1)
        })
    }

    fn is_secret_file(&self, arg: &str) -> bool {
        let arg = arg.trim_matches(|c| c == '"' || c == '\'');
        if arg.is_empty() || arg.contains("://") {
            return false;
        }
        let path = arg.trim_start_matches("./");
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.ends_with(".pub") {
            return false;
        }
        if self.by_name.is_match(name) {
            return true;
        }
        // Try every suffix of the path, so `~/.aws/credentials` and
        // `apps/web/.kube/config` match `.kube/config`.
        let mut rest = path.trim_start_matches('~').trim_start_matches('/');
        loop {
            if self.by_path.is_match(rest) {
                return true;
            }
            match rest.split_once('/') {
                Some((_, tail)) => rest = tail,
                None => return false,
            }
        }
    }
}

/// `-o yaml`, `-ojson`, `--output=jsonpath=...`: anything that prints the data.
fn output_shows_data(words: &[&str]) -> bool {
    let mut it = words.iter().peekable();
    while let Some(w) = it.next() {
        let value = if *w == "-o" || *w == "--output" {
            it.peek().copied().copied()
        } else if let Some(v) = w.strip_prefix("--output=") {
            Some(v)
        } else {
            w.strip_prefix("-o").map(|v| v.trim_start_matches('='))
        };
        if let Some(v) = value
            && !v.is_empty()
            && !matches!(v, "name" | "wide")
        {
            return true;
        }
    }
    false
}

fn mentions_secret_var(arg: &str) -> bool {
    let mut rest = arg;
    while let Some(i) = rest.find('$') {
        rest = &rest[i + 1..];
        let rest_name = rest.trim_start_matches('{');
        let name: String = rest_name
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && name_looks_secret(&name) {
            return true;
        }
    }
    false
}

fn is_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Drop `sudo -u x`, `VAR=1`, `time`, `nice -n 5` and similar from the front.
fn strip_prefixes(tokens: &[String]) -> &[String] {
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        let bin = basename(t);
        if is_assignment(t) {
            i += 1;
        } else if PREFIXES.contains(&bin) {
            i += 1;
            // Their options, and the values of the options that take one.
            while i < tokens.len() && tokens[i].starts_with('-') {
                let opt = tokens[i].as_str();
                i += 1;
                if matches!(
                    opt,
                    "-u" | "-g" | "-C" | "-n" | "-h" | "-p" | "-c" | "-U" | "-D"
                ) {
                    i += 1;
                }
            }
        } else {
            break;
        }
    }
    &tokens[i.min(tokens.len())..]
}

/// Split a shell command line into simple commands (on `|`, `&&`, `||`, `;`,
/// `&`, newlines, subshell and command-substitution brackets), each a list of
/// words with quotes removed.
fn segments(command: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars().peekable();
    let finish_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => {
                            if let Some(n) = chars.next() {
                                word.push(n);
                            }
                        }
                        _ => word.push(q),
                    }
                }
            }
            '\\' => {
                if let Some(n) = chars.next()
                    && n != '\n'
                {
                    word.push(n);
                    in_word = true;
                }
            }
            '|' | '&' | ';' | '\n' | '(' | ')' | '`' => {
                // `2>&1` is a redirection, not a separator.
                if c == '&' && word.ends_with('>') {
                    word.push(c);
                    continue;
                }
                if c == '(' && word.ends_with('$') {
                    word.pop();
                    if word.is_empty() {
                        in_word = false;
                    }
                }
                finish_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            '<' if !in_word => {
                // Input redirection: `cat < .env` reads the file.
                if chars.peek() == Some(&'(') {
                    continue;
                }
            }
            c if c.is_whitespace() => finish_word(&mut word, &mut in_word, &mut words),
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    finish_word(&mut word, &mut in_word, &mut words);
    if !words.is_empty() {
        out.push(words);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn splits_commands() {
        let segs = segments("FOO=1 sudo env | grep -i 'a b' && echo $(cat .env) 2>&1; ls");
        let flat: Vec<Vec<&str>> = segs
            .iter()
            .map(|s| s.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            flat,
            vec![
                vec!["FOO=1", "sudo", "env"],
                vec!["grep", "-i", "a b"],
                vec!["echo"],
                vec!["cat", ".env"],
                vec!["2>&1"],
                vec!["ls"],
            ]
        );
    }
}
