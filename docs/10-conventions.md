# 10 Conventions

## Rust

- Edition 2024, latest stable toolchain.
- `#![forbid(unsafe_code)]` in every crate.
- No `unwrap` or `expect` in library crates; `thiserror` for library errors, `anyhow` only in the binary.
- Secrets in `secrecy::SecretString`; no secret ever in logs, errors or `Debug` output.
- Pure functions for parsing, validation, ID generation, conflict checks and brief rendering, each with unit tests. File and network I/O stay at the edges.
- `cargo fmt`, and `cargo clippy --workspace --all-targets -- -D warnings` clean before every commit.
- Snapshot tests (`insta`) for CLI output and MCP tool schemas and descriptions.

## Commits

- Small commits, one change each, imperative subject with a scope: `decisions: refuse accepted decisions without a basis`.
- Docs and decisions change in the same commit as the code they describe.
- Reference decisions by ID in the commit body when a commit implements one.

## Docs

- NZ English. No em dashes.
- Docs are the source of truth. If code and docs disagree, one of them is a bug; fix it in the same commit.
- Public text and examples use neutral placeholders (example.com, USD, a generic company), never a real customer.

## Releases

Releases are built by `.github/workflows/release.yml` and run only when a version tag is pushed, never on ordinary pushes. To cut one:

1. Bump `version` in the root `Cargo.toml` and commit it.
2. Tag that commit `v<version>` (for example `v0.2.0`) and push the tag: `git tag v0.2.0 && git push origin v0.2.0`.

The workflow checks the tag matches the workspace version, runs the tests, builds `inloop` for Linux (x86_64, arm64), macOS (Apple silicon, Intel) and Windows (x86_64), and creates a GitHub release with the archives, a `SHA256SUMS` file and generated notes. A tag with a suffix (`v0.2.0-rc.1`) makes a prerelease.

## Recording decisions

Use `inloop dec new` (or the `decision_new` MCP tool): it picks a free ID, checks every worktree and branch, and writes through the redactor. Find decisions with `inloop search` or `decision_search`.

The manual steps below are the fallback when the binary isn't available. They predate the CLI and were how the first decisions were written (D-NPET).

1. **Is it really a decision?** Only if a human on the project made it, or it's unmistakably clear from what they said. Quote them in `basis` and set `status: accepted`. If you chose it yourself, set `status: proposed`, `decided_by: agent`, and say in your final message that it needs the human's call.
2. **Make an ID:**

   ```sh
   id="D-$(LC_ALL=C tr -dc '0123456789ABCDEFGHJKMNPQRSTVWXYZ' </dev/urandom | head -c4)"; echo "$id"
   ```

3. **Check it's unused** in this worktree and every other one:

   ```sh
   git worktree list --porcelain | awk '/^worktree /{print $2}' | while read -r w; do ls "$w/docs/decisions" 2>/dev/null; done | grep -c "$id"
   ```

   Anything but `0` means pick another. So does an ID that is all digits (D-P5PT).
4. **Write** `docs/decisions/<id>-<short-slug>.md` with the frontmatter and the Context, Decision and Consequences sections.
5. **To find decisions**, search the folder: `grep -ril "<term>" docs/decisions`.

When phase 1 lands, `inloop lint` checks every hand-written decision and anything it flags gets fixed.
