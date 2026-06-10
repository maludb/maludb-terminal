# Ubuntu Linux Build

This is the first Linux target for `malu`. Build on Ubuntu for the v1 Linux
package; cross-compiling from macOS can come later after the native path is
stable.

## Prerequisites

```bash
sudo apt-get update
sudo apt-get install -y build-essential ca-certificates curl pkg-config libdbus-1-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup target add x86_64-unknown-linux-gnu
```

`reqwest` uses Rustls, so OpenSSL development packages are not required for
HTTP. `libdbus-1-dev` supports the Linux keyring backend. On headless servers,
use file token storage instead:

```bash
malu set-token malu_... --store file
```

## Build And Test

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --target x86_64-unknown-linux-gnu
./target/x86_64-unknown-linux-gnu/release/malu --help
```

## Package

```bash
scripts/package-release.sh --target x86_64-unknown-linux-gnu
(cd dist && sha256sum -c malu-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256)
```

The package contains:

- `bin/malu`
- `README.md`
- `install.sh`

Install from an unpacked bundle:

```bash
tar -xzf dist/malu-0.1.0-x86_64-unknown-linux-gnu.tar.gz -C /tmp
cd /tmp/malu-0.1.0-x86_64-unknown-linux-gnu
sudo ./install.sh
malu --help
```

Override the install prefix when needed:

```bash
PREFIX="$HOME/.local" ./install.sh
```

## Smoke Test

For hosted API smoke testing:

```bash
malu profile create maludb-api \
  --api-url https://api.maludb.org \
  --user-name Craig \
  --project "maludb api" \
  --namespace default
malu set-token malu_... --store file
malu subjects add "MaluDB API"
malu smoke full
```

For local API smoke testing, start the API from `/Users/user/maludb-python-simple`
or the equivalent Ubuntu checkout, then create the profile with:

```bash
malu profile create local-api --api-url http://localhost:8000 --namespace default
```
