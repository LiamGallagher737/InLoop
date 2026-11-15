# 07 Config

Each project has one `inloop.toml` at the repo root, committed. It holds everything project-specific, so InLoop's code and defaults stay general (D-0217). InLoop's own file is [`../inloop.toml`](../inloop.toml); keep it and this doc in step.

Personal settings live outside the repo, never in `inloop.toml`: `~/.config/inloop/config.toml` (`data_dir` to move the local session store; `[s3] access_key_id`, `secret_access_key`) and the project keys in `~/.config/inloop/keys/`. `INLOOP_CONFIG_DIR` and `INLOOP_DATA_DIR` move both (D-360T).

## Sections

### `[project]`

| Key | Default | Meaning |
|---|---|---|
| `name` | repo directory name | Shown in output |
| `main_branch` | detected from `origin/HEAD`, else `main` | What "merged" means |

### `[decisions]`

| Key | Default | Meaning |
|---|---|---|
| `dir` | `docs/decisions` | Where decision files live (D-SJBG) |
| `id_prefix` | `D` | The letter before the dash |
| `id_length` | `4` | Crockford base32 characters after the dash |
| `lint_references` | `["**/*.md"]` | Globs of files whose `D-XXXX` references `inloop lint` checks |
| `example_ids` | none | IDs used as examples in docs, which lint doesn't expect to exist |

### `[checks]`

| Key | Default | Meaning |
|---|---|---|
| `commands` | none | Commands an agent runs before saying work is done; also included in PM briefs |

### `[sessions]`

| Key | Default | Meaning |
|---|---|---|
| `backend` | `local` | `local` or `s3` (docs/03) |
| `bucket`, `endpoint`, `region` | none | For `s3`; credentials come from the personal config or the environment (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`) |
| `prefix` | project name | Key prefix inside the bucket |

### `[redaction]`

| Key | Default | Meaning |
|---|---|---|
| `known_value_files` | `[".env", ".env.*"]` | Files whose values are always redacted |
| `patterns` | none | Extra regex rules, each with a `kind` label |
| `personal_data` | `false` | Turn on rules for emails, phone numbers and similar |

### `[pm]`

The optional PM layer (docs/05). Everything here is ignored unless `enabled = true`.

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | Register the PM commands and MCP toolset |
| `brief_template` | `.inloop/brief.md` | Template for generated child briefs |
| `conflict_groups` | none | Array of `{ name, paths }`; paths are globs only one running task may touch |
