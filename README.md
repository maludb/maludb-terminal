# maludb-terminal

`malu` is a command-first Rust terminal CLI for sending notes, documents, and smoke-test
workflows to the MaluDB API.

## Current Slice

Implemented commands:

```bash
malu set-api https://api.maludb.org
malu set-token malu_...
malu set-token malu_... --store file

malu token mint \
  --pg-dbname maludb \
  --pg-user craig \
  --pg-password '...' \
  --device-name macbook

malu profile create maludb-api \
  --api-url https://api.maludb.org \
  --user-name Craig \
  --project "maludb api" \
  --namespace default
malu profile use maludb-api
malu profile list
malu profile show
malu profile delete old-project

malu subjects add "FastAPI"
malu subjects list
malu subjects clear

malu hints add "This is about API smoke testing"
malu hints list
malu hints clear

malu get config
malu get subjects --query FastAPI --limit 5 --json
malu get projects --query "maludb api"
malu get documents --with attributes

malu note "Starting to debug the maludb api"
malu doc push ./debug-log.md
malu chat push --source codex ~/.codex/sessions/YYYY/MM/DD/session.jsonl
malu chat push --source claude-code ~/.claude/projects/project/session.jsonl

malu smoke health
malu smoke config
malu smoke note
malu smoke document ./sample.md
malu smoke search --query "debug API" --subject "FastAPI"
malu smoke full

malu sync push
malu sync pull
malu sync status
malu sync diff

malu completions bash > malu.bash
```

Notes use `POST /v1/memory/ingest` with a context preamble and active profile
hints. Document pushes and chat log uploads use `POST /v1/memory/documents`,
pass active subjects as API subjects, and store active hints in metadata. Chat
logs from Codex and Claude Code are normalized into readable transcripts before
upload and tagged with their original source in metadata.

Tokens are stored in the platform keyring by default. Use `--store file` on
headless systems; file credentials are stored separately from `config.toml` and
use strict permissions on Unix.

Sync v1 stores portable CLI settings in an internal MaluDB note named
`malu-cli-settings` with type `malu_cli_settings`. Raw API tokens are never
included in the synced settings blob.

## Install

From this repository:

```bash
cargo install --path .
```

The CLI currently targets macOS and Ubuntu Linux, with Windows kept in the code
path through `directories` and platform keyring support.

On a new Ubuntu server, start with the Ubuntu runbook:

```bash
git clone https://github.com/maludb/maludb-terminal.git
cd maludb-terminal
```

Then follow [`docs/ubuntu.md`](docs/ubuntu.md) to install prerequisites, build,
package, install, and run smoke tests.

## Local Development

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Release Bundles

Build a Unix release bundle for the current host or an installed Rust target:

```bash
scripts/package-release.sh --target x86_64-unknown-linux-gnu
```

Ubuntu-specific build, install, and smoke-test notes are in
[`docs/ubuntu.md`](docs/ubuntu.md).
