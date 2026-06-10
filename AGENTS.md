## Superpowers System

<EXTREMELY_IMPORTANT>
You have superpowers. Superpowers teach you new skills and capabilities. RIGHT NOW run:

```bash
~/.codex/superpowers/.codex/superpowers-codex bootstrap
```

Follow the instructions it returns.
</EXTREMELY_IMPORTANT>

## Project Summary

This repository is for `malu`, a cross-platform CLI for sending notes, documents, context, and smoke-test workflows to the MaluDB API.

The current API source repo is `/Users/user/maludb-python-simple`. Treat that repo as reference material unless the user explicitly asks to change it. The hosted API is `https://api.maludb.org`, and local development usually runs the FastAPI server at `http://localhost:8000`.

The first version of the CLI must exercise the full memory pipeline from the beginning. Quick notes and document uploads should go through memory ingest behavior, not only the simple note CRUD endpoint. This tool will also become the ongoing smoke-test harness for API features and workflows.

## Recommended Tech Stack

Use Rust for the CLI.

Core crates:

- `clap` for subcommands, flags, help text, completions, and typed parsing.
- `reqwest` with blocking, JSON, and multipart support for API calls.
- `serde`, `serde_json`, and `toml` for config and payloads.
- `directories` for platform-specific config, data, and cache paths.
- `keyring` for secure token storage through macOS Keychain, Windows Credential Manager, and Linux Secret Service when available.
- `chrono` or `time` for timestamps and timezone-aware context.
- `mime_guess` for document upload media types.
- `anyhow` for CLI-level errors, with `thiserror` for structured internal errors if useful.

Start with a command-first CLI. Do not build a terminal UI until the command model, profiles, and smoke-test workflow are stable.

## API Endpoints To Target

Important existing endpoints in `/Users/user/maludb-python-simple`:

- `GET /health`: server health.
- `POST /v1/tokens`: mint an API token from Postgres credentials.
- `GET /v1/subjects`: fetch subjects for context selection.
- `GET /v1/projects`: fetch projects.
- `POST /v1/memory/documents`: preferred v1 path for notes and documents because it uploads, chunks, extracts, embeds, and ingests.
- `POST /v1/memory/search`: smoke-test search after ingest.
- `GET /v1/memory/config`: check memory model configuration.
- `POST /v1/notes`, `GET /v1/notes`, `PATCH /v1/notes/{id}`: acceptable v1 storage path for synced CLI settings if no dedicated client-settings endpoint exists yet.
- `GET /v1/llm/catalog`: seeded model catalog (provider × model × task) with the caller's key/choice state.
- `GET/PUT/DELETE /v1/llm/providers/{provider}`: the user's LLM provider API keys (key values never returned).
- `GET/PUT/DELETE /v1/llm/models/{task}`: the user's task → model choices (`extract`, `skill_extract`, `embed`).

The API returns standard errors shaped like:

```json
{"error":{"code":"...","message":"..."}}
```

The CLI should surface these clearly and preserve the server error code.

## Profile Model

Profiles are the core abstraction. Users work across multiple projects and machines, so a profile should represent a saved working context.

Each profile should include:

- API URL.
- Token lookup key, not the raw token when secure storage is available.
- User identity, such as name and role.
- Project name or identifier.
- Memory namespace.
- Optional legacy note-model override (`model`); unset means the server resolves the user's `malu llm use` choice.
- Active subjects.
- Active hints.
- Smoke-test defaults.
- Updated timestamps for sync and conflict handling.

Example local config shape:

```toml
active_profile = "maludb-api"

[profiles.maludb-api]
api_url = "https://api.maludb.org"
token_key = "maludb-api"
user_name = "Craig"
project = "maludb api"
namespace = "default"
subjects = ["MaluDB API", "FastAPI", "memory pipeline"]
hints = [
  "This work is related to debugging and improving the hosted MaluDB API",
  "Prefer software engineering interpretation of notes"
]
```

## Command Model

Initial command shape:

```bash
malu set-api https://api.maludb.org
malu set-token malu_...

malu profile create maludb-api
malu profile use maludb-api
malu profile list
malu profile show
malu profile delete old-project

malu get config
malu get subjects
malu get projects
malu get documents

malu llm catalog
malu llm providers
malu llm set-key <provider>
malu llm remove-key <provider>
malu llm models
malu llm use <model> [--task extract|skill-extract|embed]
malu set-model <model>   # legacy per-profile override for `malu note`

malu subjects add "FastAPI"
malu subjects clear
malu subjects list

malu hints add "This is about API smoke testing"
malu hints clear
malu hints list

malu note "Starting to debug the maludb api"
malu doc push ./debug-log.md

malu smoke health
malu smoke config
malu smoke note
malu smoke document ./sample.md
malu smoke search --subject "maludb api"
malu smoke full

malu sync push
malu sync pull
malu sync status
malu sync diff
```

`set-api`, `set-token`, `subjects add`, and `hints add` apply to the active profile by default.

## Context Assembly

`malu note` should enrich short user input before sending it through the memory pipeline.

Example generated text:

```text
Context:
- User: Craig
- Time: 2026-06-09T...
- Project: maludb api
- Subjects: MaluDB API, FastAPI
- Hints: This note is about debugging the hosted API

Note:
Starting to debug the maludb api
```

For `POST /v1/memory/documents`, map active subjects directly to the API `subjects` array. Also store hints in metadata and include a concise context preamble in the submitted text, because the current endpoint does not expose a dedicated `hints` field.

## Sync Design

Local settings should be portable across computers through MaluDB.

Do sync:

- Profiles.
- User context.
- Project context.
- Subjects.
- Hints.
- Namespace defaults.
- Smoke-test defaults.

Do not sync raw API tokens as plain config. On a new computer the user should bootstrap with:

```bash
malu set-api https://api.maludb.org
malu set-token malu_...
malu sync pull
malu profile use maludb-api
```

For v1, use an internal note as remote settings storage if no dedicated API endpoint exists:

- `POST /v1/notes` with `type = "malu_cli_settings"`.
- `GET /v1/notes?type=malu_cli_settings&q=malu-cli-settings`.
- `PATCH /v1/notes/{id}` for updates.

The synced settings blob should include `schema_version`, `updated_at`, `device_id`, and per-profile `updated_at` values. V1 conflict policy can be "newer wins" with a warning. Later versions can add merge prompts.

## Platform Support

V1 targets:

1. macOS, because this project starts on a Mac with Codex.
2. Ubuntu Linux, because the API and much destination code run there.
3. Windows, designed in from the start and verified after macOS and Ubuntu.

Use `directories` for config paths:

- macOS: `~/Library/Application Support/...`
- Ubuntu: `~/.config/malu/config.toml`, `~/.local/share/malu/`, `~/.cache/malu/`
- Windows: `%APPDATA%\MaluDB\malu\config.toml`

Use `keyring` where available. On Ubuntu headless systems, support an explicit file fallback:

```bash
malu set-token malu_... --store file
```

The fallback credentials file must use strict permissions, such as mode `0600`.

Build targets to plan for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

## Smoke Testing Goal

`malu smoke full` should run a real workflow against the active profile:

1. Check `/health`.
2. Check authenticated API access.
3. Check memory config.
4. Ingest a timestamped test note through the memory pipeline.
5. Ingest a small generated document through the memory pipeline.
6. Run memory search against the ingested context.
7. Print clear pass/fail output with returned IDs and server error codes.

This command is both a user diagnostic and a regression smoke test for API releases.

## Current State

As of 2026-06-09:

- `/Users/user/maludb-python-simple` is clean and should remain unchanged unless explicitly requested.
- The local FastAPI server was started with `python3 -m uvicorn app.main:app --reload --port 8000`.
- `http://localhost:8000/health` returns `{"status":"ok"}`.
- `http://localhost:8000/docs` is the browser-friendly API preview. The root path `/` returns 404 by design.

