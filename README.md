# maludb-terminal

This application maludb-terminal `maludb` is a command-first terminal CLI for interfacing with the MaluDb memory database. It also serves as an MCP server for your Claude Code, Codex, and your MCP enabled agents to work with MaluDb. This application accesses the memory database using a MaluDb API server of your choice which you must configure after installation.

## Getting Started

New to `maludb`? This is the quickest path from nothing to your first note.

### 1. Install and verify

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.sh | sh
```

This installs `maludb` to `~/.local/bin`. Verify it runs:

```bash
maludb --help
```

If you see `command not found`, `~/.local/bin` is not on your `PATH`. Add it,
then open a new terminal:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc   # or ~/.zshrc
```

Windows, Homebrew, `cargo binstall`, and build-from-source are covered under
[Install](#install).

### 2. Get an API token

Minting a token requires your MaluDB Postgres **database name**, **user name**,
and **password**. Request one from the API with `curl`:

```bash
curl -X POST https://api.maludb.org/v1/tokens \
  -H 'Content-Type: application/json' \
  -d '{"pg_dbname":"<database name>","pg_user":"<user name>","pg_password":"<user password>"}'
```

Replace `https://api.maludb.org` with your own API server if you run one. The
response contains your token in the `token` field; it starts with `malu_`. Copy
it for the next step.

### 3. Point maludb at the API and save the token

```bash
maludb set-api https://api.maludb.org      # your API server
maludb set-token malu_...                   # the token from step 2
```

On a headless server with no system keyring, store it in a file instead:

```bash
maludb set-token malu_... --store file
```

> Tip: once `maludb` is installed, steps 2 and 3 can be done in one command —
> `maludb token mint --pg-dbname <database name> --pg-user <user name> --pg-password <user password>`
> makes the same request and saves the token for you.

### 4. Send your first note

```bash
maludb note "My first MaluDB note"
```

Run `maludb smoke full` at any time to exercise the whole pipeline end to end.

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

maludb get note --subject-like ubuntu --verb-like installation --limit 20
maludb get note --subject-like ubuntu --action install        # exact verb (or alias)
maludb get note "Install Ubuntu"                              # free text, parsed server-side
maludb get note --subject-like ubuntu --all-sources           # widen beyond notes

maludb get skill pdf-processing                              # install into ~/.claude/skills/<name>
maludb get skill pdf-processing --dest ./skills/ --force     # ...or a specific folder

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

maludb mcp                             # run as a local MCP server over stdio
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

## MCP Server

`maludb mcp` runs a local [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio, so agents like Claude Code and Codex can call a curated set of
maludb commands as tools. Each tool maps to a normal CLI invocation against your
active profile; configure the profile, token, and LLM model first, then point a
client at `maludb mcp`.

Register it with **Claude Code**:

```bash
claude mcp add maludb -- maludb mcp
```

Register it with **Codex** (`~/.codex/config.toml`):

```toml
[mcp_servers.maludb]
command = "maludb"
args = ["mcp"]
```

The exposed tools cover notes and uploads (`note`, `doc_push`, `chat_push`),
context (`subjects_add`/`hints_add`, `subjects_list`/`hints_list`,
`profile_list`/`profile_show`), reads (`get_config`, `get_subjects`,
`get_projects`, `get_documents`, `get_note`, `llm_catalog`, `llm_models`,
`skill_list`), skills (`skill_add`, `skill_pull`, `get_skill`), sync
(`sync_push`/`pull`/`status`/`diff`), and smoke tests
(`smoke_health`/`config`/`search`/`full`).

For safety the server **does not** expose credential or secret mutation
(`set-token`, `token mint`, `llm set-key`, profile/token deletion). Run those by
hand once when setting the machine up. Each tool call re-executes the `maludb`
binary as a child process and returns its captured output, so a failing command
can never corrupt the protocol stream.

## Install

Prebuilt binaries are published for each release, so installing on a new
machine needs neither Rust nor a compile step.

### One-line install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.sh | sh
```

Installs the latest release to `~/.local/bin/maludb` (see
[Getting Started](#getting-started) if it is not found on your `PATH`). Pin a
version or change the location:

```bash
curl -fsSL https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.sh \
  | sh -s -- --version v0.2.0 --bin-dir /usr/local/bin
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.ps1 | iex
```

### Homebrew (macOS / Linux)

```bash
brew install maludb/tap/maludb
```

### cargo-binstall

If you already have [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall),
it pulls the same prebuilt binary:

```bash
cargo binstall maludb
```

Supported targets: `x86_64`/`aarch64` Linux (glibc), `x86_64`/`aarch64` macOS,
and `x86_64` Windows. Each release also ships `.tar.gz`/`.zip` archives plus
`.sha256` checksums on the
[Releases page](https://github.com/maludb/maludb-terminal/releases) for manual
installs.

### Build from source

Building `maludb` yourself (instead of installing a prebuilt binary) needs three
things: a recent Rust toolchain, a C compiler/linker, and the system libraries
the keyring credential backend links against.

**1. Rust toolchain (1.85 or newer).** The crate uses the 2024 edition, which
requires Rust **1.85+**. Install or update with [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # install
rustup update                                                    # or update an existing toolchain
rustc --version                                                  # confirm >= 1.85.0
```

**2. C toolchain and system libraries.**

- **Debian / Ubuntu:**

  ```bash
  sudo apt-get update
  sudo apt-get install -y build-essential pkg-config libdbus-1-dev git
  ```

- **Fedora / RHEL:**

  ```bash
  sudo dnf install -y gcc pkgconf-pkg-config dbus-devel git
  ```

- **Other Linux:** install the equivalent of a C compiler, `pkg-config`, and the
  D-Bus development headers (`libdbus-1-dev`).

- **macOS:** install the Command Line Tools (provides `clang` and the linker):

  ```bash
  xcode-select --install
  ```

- **Windows:** install the
  [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  ("Desktop development with C++") for the MSVC linker.

`reqwest` is built with Rustls, so OpenSSL development packages are **not**
required. The `libdbus-1-dev` / `dbus-devel` package backs the Linux keyring
credential store (`keyring` → `dbus-secret-service`); without it the build fails
to link. On headless boxes you can avoid the keyring at runtime with file token
storage (`maludb set-token ... --store file`).

**3. Clone, then build and install.**

```bash
git clone https://github.com/maludb/maludb-terminal.git
cd maludb-terminal
cargo install --path .          # installs `maludb` into ~/.cargo/bin
# ...or build without installing:
cargo build --release           # binary at target/release/maludb
```

The Ubuntu runbook ([`docs/ubuntu.md`](docs/ubuntu.md)) also covers building a
release bundle and running smoke tests from source.

## Local Development

Needs the same prerequisites as [Build from source](#build-from-source) (Rust
1.85+, a C toolchain, and the D-Bus dev libraries on Linux). From a clone:

```bash
cargo build                                                   # compile
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Releases

Pushing a `vX.Y.Z` tag triggers
[`.github/workflows/release.yml`](.github/workflows/release.yml), which builds
each target on a native runner, packages it, and publishes the archives and
checksums to a GitHub Release. To cut a release:

```bash
# bump version in Cargo.toml first, commit, then:
git tag v0.2.0
git push origin v0.2.0
```

You can also build a bundle locally for the current host or an installed target:

```bash
scripts/package-release.sh --target x86_64-unknown-linux-gnu   # macOS / Linux
./scripts/package-release.ps1 -Target x86_64-pc-windows-msvc   # Windows (PowerShell)
```

Ubuntu-specific build, install, and smoke-test notes are in
[`docs/ubuntu.md`](docs/ubuntu.md).

### Homebrew tap (one-time setup)

The release workflow keeps a Homebrew formula up to date when a tap is wired up:

1. Create a public repo `maludb/homebrew-tap`.
2. Create a Personal Access Token with `contents: write` on that repo and add it
   to this repo's Actions secrets as `HOMEBREW_TAP_TOKEN`.

On the next release the `homebrew` job regenerates `Formula/maludb.rb` in the tap
(via [`scripts/update-homebrew-formula.sh`](scripts/update-homebrew-formula.sh)),
enabling `brew install maludb/tap/maludb`. Without the secret, the job logs a
notice and skips — the rest of the release still publishes.

## License

Released under the [MIT License](LICENSE). The packaging scripts bundle the
`LICENSE` file into every release archive automatically.
