# maludb-cli

`malu` is a command-first Rust CLI for sending notes, documents, and smoke-test
workflows to the MaluDB API.

## Current Slice

Implemented commands:

```bash
malu set-api https://api.maludb.org
malu set-token malu_... --store file

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
malu get subjects
malu get projects
malu get documents

malu note "Starting to debug the maludb api"
malu doc push ./debug-log.md

malu smoke health
malu smoke config
malu smoke note
malu smoke document ./sample.md
malu smoke search --query "debug API" --subject "FastAPI"
malu smoke full
```

Notes and document pushes use `POST /v1/memory/documents`, include a context
preamble in the submitted text, pass active subjects as API subjects, and store
active hints in metadata.

## Local Development

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
