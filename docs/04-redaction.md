# 04 Redaction

Everything InLoop stores goes through one redactor first: transcripts, summaries, decisions, board records, reports, logs (D-G1R6). If redaction fails, nothing is written.

## The gate is a type

The storage layer only accepts `Redacted<T>`. Only the `redact` crate can construct one, by running the redactor (D-VNK6). So there is no write path that can skip redaction by accident: a new kind of record that isn't redacted won't compile.

`Redacted<T>` implements none of `Deserialize`, `DerefMut`, `From` or `Default`: each would be a second way to build or change one without the redactor. Data read back from storage is plain `T`, and goes through the redactor again before it is stored again.

The gate and the first two layers ship in phase 1, so decisions are redacted from the first release (D-VH2N). The other layers follow in phase 2.

```rust
pub struct Redacted<T>(T);   // no public constructor

impl Redactor {
    pub fn redact<T: Redactable>(&self, value: T) -> Result<Redacted<T>, RedactError>;
}
```

## Layers

The redactor runs these in order. Each catches things the others miss.

1. **Known values.** The strongest layer. InLoop loads the real secret values it can find (the project's `.env` files, other files listed in `known_value_files`, environment variables with secret-looking names) and redacts exact matches. It also matches their common encodings: base64, URL-encoded and JSON-escaped. No false positives, and it catches secrets that fit no pattern.
2. **Provider patterns.** Known token formats: `sk-ant-`, `ghp_`/`github_pat_`, `AKIA`, Stripe `sk_live_`/`rk_live_`, Meta `EAA…`, Google `ya29.`, Slack `xox?-`, private key blocks, JWTs and so on. Reuse gitleaks' MIT-licensed ruleset rather than writing patterns from scratch, and record it in a third-party notices file.
3. **Context plus entropy.** High-entropy strings after `Authorization:`, `Bearer`, `token=`, `password`, `secret`, `api_key` and similar.
4. **Whole outputs.** The output of commands like `env`, `printenv`, `cat .env` and `security find-generic-password` is dropped entirely, not cleaned.
5. **Project rules.** Extra patterns from `inloop.toml`, including optional rules for personal and customer data (emails, phone numbers, names of customer accounts). A project that handles other people's data, like an ad platform, should turn these on.

Redacted text becomes `[REDACTED:<kind>]`, for example `[REDACTED:github-token]`, so a reader can tell something was there and what sort.

## Fail closed

- If the redactor errors on any input, that input is not written. Capture logs the failure (without content) and reports it in `inloop session status`.
- Secret values loaded for layer 1 are held in `secrecy::SecretString`, never logged, never in `Debug` output, never written anywhere.

## Test corpus

- A corpus of fake secrets shaped like real ones, embedded in realistic transcripts, tool output and code.
- Tests measure how many are caught and fail the build if any known case is missed.
- Every miss found in real use becomes a new fixture.
- False positives matter too (redacting a commit hash ruins a transcript), so the corpus includes look-alikes that must survive.

## After the fact

- `inloop scan` re-runs the current redactor over everything stored, to catch what an older version missed.
- `inloop session purge` removes a session completely (docs/03).
