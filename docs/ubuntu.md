# Ubuntu Linux Build And Install

This runbook covers a fresh Ubuntu server install for `malu`, including source
checkout, verification, packaging, local installation, and smoke testing.

The Linux release target is:

```bash
x86_64-unknown-linux-gnu
```

## Fresh Server Setup

Install OS packages:

```bash
sudo apt-get update
sudo apt-get install -y build-essential ca-certificates curl git pkg-config libdbus-1-dev
```

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup target add x86_64-unknown-linux-gnu
```

Clone the production repository:

```bash
git clone https://github.com/maludb/maludb-terminal.git
cd maludb-terminal
```

If you are testing an unreleased branch, clone it explicitly:

```bash
git clone -b <branch-name> https://github.com/maludb/maludb-terminal.git
cd maludb-terminal
```

## Verify From Source

Run the same checks used before packaging:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --target x86_64-unknown-linux-gnu
./target/x86_64-unknown-linux-gnu/release/malu --help
```

`reqwest` uses Rustls, so OpenSSL development packages are not required for
HTTP. `libdbus-1-dev` supports the Linux keyring backend. On headless servers,
use file token storage instead of the keyring:

```bash
malu set-token malu_... --store file
```

## Build A Release Bundle

Create the Linux tarball and verify its checksum:

```bash
scripts/package-release.sh --target x86_64-unknown-linux-gnu
(cd dist && sha256sum -c malu-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256)
```

The bundle contains:

- `bin/malu`
- `README.md`
- `install.sh`

Generated files under `dist/` are release artifacts and should not be committed.

## Install From The Bundle

Install for the current user:

```bash
tmpdir="$(mktemp -d)"
tar -xzf dist/malu-0.1.0-x86_64-unknown-linux-gnu.tar.gz -C "$tmpdir"
PREFIX="$HOME/.local" "$tmpdir/malu-0.1.0-x86_64-unknown-linux-gnu/install.sh"
export PATH="$HOME/.local/bin:$PATH"
malu --help
```

Persist the user-local install path for future shells:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.bashrc" \
  || echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
```

Install system-wide instead:

```bash
tmpdir="$(mktemp -d)"
tar -xzf dist/malu-0.1.0-x86_64-unknown-linux-gnu.tar.gz -C "$tmpdir"
cd "$tmpdir/malu-0.1.0-x86_64-unknown-linux-gnu"
sudo ./install.sh
malu --help
```

The system-wide installer defaults to `/usr/local/bin/malu`. Override the prefix
when needed:

```bash
sudo PREFIX=/opt/malu ./install.sh
```

## Configure Hosted API Access

Create a profile and store the token in the file credential store on headless
Ubuntu:

```bash
malu profile create maludb-api \
  --api-url https://api.maludb.org \
  --user-name Craig \
  --project "maludb api" \
  --namespace default
malu set-token malu_... --store file
malu subjects add "MaluDB API"
```

Set up the LLM extraction model for `malu note` (one-time; the key is stored
server-side, never in local files):

```bash
malu llm catalog            # see what the server offers
malu llm set-key openai     # paste the key at the hidden prompt
malu llm use gpt-4o         # extraction model for notes
malu llm models             # verify the task -> model choices
```

On headless servers the key can be piped instead of typed:
`printf '%s\n' "$OPENAI_API_KEY" | malu llm set-key openai`.

Run smoke tests:

```bash
malu smoke health
malu smoke config
malu smoke full
```

## Configure Local API Access

If the API is running locally on the Ubuntu server:

```bash
malu profile create local-api \
  --api-url http://localhost:8000 \
  --namespace default
malu profile use local-api
malu set-token malu_... --store file
malu smoke health
```

## Update And Rebuild

To update an existing checkout:

```bash
git pull --ff-only
cargo test
scripts/package-release.sh --target x86_64-unknown-linux-gnu
```
