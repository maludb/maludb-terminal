# Ubuntu Linux Build And Install

For most installs you do **not** need to build from source. The fastest path on
a fresh Ubuntu machine is the prebuilt-binary installer:

```bash
curl -fsSL https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
maludb --help
```

This downloads the release archive for your architecture
(`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`), verifies its
checksum, and installs `maludb` to `~/.local/bin`. No Rust toolchain or compile
step is required. Skip ahead to [Configure Hosted API Access](#configure-hosted-api-access).

The rest of this runbook covers building from source, which you only need when
hacking on `maludb` itself or producing release artifacts.

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
./target/x86_64-unknown-linux-gnu/release/maludb --help
```

`reqwest` uses Rustls, so OpenSSL development packages are not required for
HTTP. `libdbus-1-dev` supports the Linux keyring backend. On headless servers,
use file token storage instead of the keyring:

```bash
maludb set-token malu_... --store file
```

## Build A Release Bundle

Create the Linux tarball and verify its checksum:

```bash
scripts/package-release.sh --target x86_64-unknown-linux-gnu
(cd dist && sha256sum -c maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256)
```

The bundle contains:

- `bin/maludb`
- `README.md`
- `install.sh`

Generated files under `dist/` are release artifacts and should not be committed.

## Install From The Bundle

Install for the current user:

```bash
tmpdir="$(mktemp -d)"
tar -xzf dist/maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz -C "$tmpdir"
PREFIX="$HOME/.local" "$tmpdir/maludb-0.1.0-x86_64-unknown-linux-gnu/install.sh"
export PATH="$HOME/.local/bin:$PATH"
maludb --help
```

Persist the user-local install path for future shells:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.bashrc" \
  || echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
```

Install system-wide instead:

```bash
tmpdir="$(mktemp -d)"
tar -xzf dist/maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz -C "$tmpdir"
cd "$tmpdir/maludb-0.1.0-x86_64-unknown-linux-gnu"
sudo ./install.sh
maludb --help
```

The system-wide installer defaults to `/usr/local/bin/maludb`. Override the prefix
when needed:

```bash
sudo PREFIX=/opt/maludb ./install.sh
```

## Configure Hosted API Access

Create a profile and store the token in the file credential store on headless
Ubuntu:

```bash
maludb profile create maludb-api \
  --api-url https://api.maludb.org \
  --user-name Craig \
  --project "maludb api" \
  --namespace default
maludb set-token malu_... --store file
maludb subjects add "MaluDB API"
```

Set up the LLM extraction model for `maludb note` (one-time; the key is stored
server-side, never in local files):

```bash
maludb llm catalog            # see what the server offers
maludb llm set-key openai     # paste the key at the hidden prompt
maludb llm use gpt-4o         # extraction model for notes
maludb llm models             # verify the task -> model choices
```

On headless servers the key can be piped instead of typed:
`printf '%s\n' "$OPENAI_API_KEY" | maludb llm set-key openai`.

Run smoke tests:

```bash
maludb smoke health
maludb smoke config
maludb smoke full
```

## Configure Local API Access

If the API is running locally on the Ubuntu server:

```bash
maludb profile create local-api \
  --api-url http://localhost:8000 \
  --namespace default
maludb profile use local-api
maludb set-token malu_... --store file
maludb smoke health
```

## Update And Rebuild

To update an existing checkout:

```bash
git pull --ff-only
cargo test
scripts/package-release.sh --target x86_64-unknown-linux-gnu
```
