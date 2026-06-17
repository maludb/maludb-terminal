# maludb-terminal

`maludb` is a command-first Rust terminal CLI for sending notes, documents, and smoke-test
workflows to the MaluDB API.

## Current Slice

Implemented commands:

```bash
maludb set-api https://api.maludb.org
maludb set-token malu_...
maludb set-token malu_... --store file

maludb token mint \
  --pg-dbname maludb \
  --pg-user craig \
  --pg-password '...' \
  --device-name macbook

maludb profile create maludb-api \
  --api-url https://api.maludb.org \
  --user-name Craig \
  --project "maludb api" \
  --namespace default
maludb profile use maludb-api
maludb profile list
maludb profile show
maludb profile delete old-project

maludb subjects add "FastAPI"
maludb subjects list
maludb subjects clear

maludb hints add "This is about API smoke testing"
maludb hints list
maludb hints clear

maludb get config
maludb get subjects --query FastAPI --limit 5 --json
maludb get projects --query "maludb api"
maludb get documents --with attributes

maludb llm catalog                     # models the server offers, per task
maludb llm providers                   # which providers you have a key stored for
maludb llm set-key openai              # key read from a hidden prompt (or stdin)
maludb llm remove-key openai
maludb llm models                      # current task -> model choices
maludb llm use gpt-4o                  # extraction model (default --task extract)
maludb llm use text-embedding-3-small --task embed
maludb set-model chatgpt-4o            # legacy: pin the model sent with `maludb note`

maludb note "Starting to debug the maludb api"
maludb note --debug "The wednesday meeting is about to begin"  # print the API extraction response
maludb doc push ./debug-log.md
maludb chat push --source codex ~/.codex/sessions/YYYY/MM/DD/session.jsonl
maludb chat push --source claude-code ~/.claude/projects/project/session.jsonl

maludb skill add php-htmx-auth                      # resolve a skill by name from ~/.claude/skills or ./.claude/skills
maludb skills add php-htmx-auth                      # `skills` is an alias for `skill`
maludb skill add ~/.claude/skills/pdf-processing    # ...or pass an explicit path
maludb skill push ~/.claude/skills/pdf-processing   # upload a Claude Agent Skill bundle (by path)
maludb skill push-all                               # scan ~/.claude/skills + ./.claude/skills
maludb skill list --verb extract                    # tag-aware discovery
maludb skill pull pdf-processing --dest ./skills/   # reconstruct (paths + executable bits)

maludb smoke health
maludb smoke config
maludb smoke note
maludb smoke document ./sample.md
maludb smoke search --query "debug API" --subject "FastAPI"
maludb smoke full

maludb sync push
maludb sync pull
maludb sync status
maludb sync diff

maludb completions bash > maludb.bash
```

Notes use `POST /v1/memory/ingest` with a context preamble and active profile
hints. The note's extraction model is resolved server-side from your
`maludb llm use` choice; set up once with `maludb llm set-key <provider>` +
`maludb llm use <model>`. Provider keys are stored server-side only — never in
local files. Against an older server that requires a model in the request,
pin one per profile with `maludb set-model chatgpt-4o`. Use
`maludb note --debug "..."` to print the API's full extraction response after
ingest.
Document pushes and chat log uploads use `POST /v1/memory/documents`,
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
