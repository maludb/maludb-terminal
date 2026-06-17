#!/bin/sh
# maludb installer — downloads a prebuilt release binary and installs it.
#
#   curl -fsSL https://raw.githubusercontent.com/maludb/maludb-terminal/main/install.sh | sh
#
# Options (pass with: ... | sh -s -- <options>):
#   --version <vX.Y.Z>   Install a specific release (default: latest).
#   --bin-dir <dir>      Install location (default: $HOME/.local/bin).
#   --target <triple>    Override target detection.
# Environment:
#   MALUDB_BIN_DIR       Same as --bin-dir.
set -eu

REPO="maludb/maludb-terminal"
BIN="maludb"

VERSION=""
BIN_DIR="${MALUDB_BIN_DIR:-$HOME/.local/bin}"
TARGET=""

info() { echo "maludb-install: $*"; }
err() { echo "maludb-install: error: $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  sed -n '2,14p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version requires a value}"; shift 2 ;;
    --bin-dir) BIN_DIR="${2:?--bin-dir requires a value}"; shift 2 ;;
    --target) TARGET="${2:?--target requires a value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) err "unknown option: $1 (try --help)" ;;
  esac
done

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) err "unsupported OS: $os — on Windows use install.ps1, or build from source" ;;
  esac
  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *) err "unsupported architecture: $arch" ;;
  esac
  printf '%s-%s\n' "$arch_part" "$os_part"
}

fetch_stdout() {
  if have curl; then curl -fsSL "$1"
  elif have wget; then wget -qO- "$1"
  else err "need curl or wget"; fi
}

download() {
  if have curl; then curl -fsSL "$1" -o "$2"
  elif have wget; then wget -qO "$2" "$1"
  else err "need curl or wget"; fi
}

[ -n "$TARGET" ] || TARGET="$(detect_target)"

if [ -z "$VERSION" ]; then
  info "resolving latest release..."
  VERSION="$(fetch_stdout "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$VERSION" ] || err "could not resolve the latest release tag"
fi

case "$VERSION" in v*) TAG="$VERSION" ;; *) TAG="v$VERSION" ;; esac
VNUM="${TAG#v}"

archive="maludb-$VNUM-$TARGET.tar.gz"
url="https://github.com/$REPO/releases/download/$TAG/$archive"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

info "downloading $archive ..."
download "$url" "$tmp/$archive" || err "download failed: $url"
download "$url.sha256" "$tmp/$archive.sha256" || err "checksum download failed: $url.sha256"

info "verifying checksum..."
(
  cd "$tmp"
  if have sha256sum; then
    sha256sum -c "$archive.sha256" >/dev/null 2>&1 || err "checksum verification failed"
  elif have shasum; then
    shasum -a 256 -c "$archive.sha256" >/dev/null 2>&1 || err "checksum verification failed"
  else
    info "warning: no sha256sum/shasum available; skipping checksum verification"
  fi
)

info "extracting..."
tar -xzf "$tmp/$archive" -C "$tmp"

mkdir -p "$BIN_DIR"
src="$tmp/maludb-$VNUM-$TARGET/bin/$BIN"
[ -f "$src" ] || err "binary not found in archive: $src"
if have install; then
  install -m 0755 "$src" "$BIN_DIR/$BIN"
else
  cp "$src" "$BIN_DIR/$BIN" && chmod 0755 "$BIN_DIR/$BIN"
fi

info "installed $BIN $VNUM to $BIN_DIR/$BIN"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    info "note: $BIN_DIR is not on your PATH. Add it with:"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac
