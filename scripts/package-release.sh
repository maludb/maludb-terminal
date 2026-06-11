#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/package-release.sh [OPTIONS]

Build and package a Unix maludb release tarball.

Options:
  --target TARGET     Rust target triple. Defaults to the local rustc host.
  --version VERSION   Release version. Defaults to Cargo.toml package version.
  --dist-dir DIR      Output directory. Defaults to dist.
  -h, --help          Show this help.

Environment:
  MALUDB_SKIP_BUILD=1   Skip cargo build and package MALUDB_BINARY instead.
  MALUDB_BINARY=PATH    Binary path to package when MALUDB_SKIP_BUILD=1.
USAGE
}

host_target() {
  rustc -vV | awk '/^host:/ { print $2 }'
}

cargo_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1
}

checksum_file() {
  local file="$1"
  local output="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "$file")" && sha256sum "$(basename "$file")") > "$output"
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$(dirname "$file")" && shasum -a 256 "$(basename "$file")") > "$output"
  else
    echo "error: sha256sum or shasum is required" >&2
    exit 1
  fi
}

target="$(host_target)"
version="$(cargo_version)"
dist_dir="dist"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      target="${2:?--target requires a value}"
      shift 2
      ;;
    --version)
      version="${2:?--version requires a value}"
      shift 2
      ;;
    --dist-dir)
      dist_dir="${2:?--dist-dir requires a value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$target" ]; then
  echo "error: could not determine Rust target" >&2
  exit 1
fi

if [ -z "$version" ]; then
  echo "error: could not determine package version" >&2
  exit 1
fi

if [ "${MALUDB_SKIP_BUILD:-0}" != "1" ]; then
  cargo build --release --target "$target"
fi

binary="${MALUDB_BINARY:-target/$target/release/maludb}"
if [ ! -x "$binary" ]; then
  echo "error: release binary not found or not executable: $binary" >&2
  exit 1
fi

package="maludb-$version-$target"
archive="$package.tar.gz"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

mkdir -p "$dist_dir" "$work_dir/$package/bin"
install -m 0755 "$binary" "$work_dir/$package/bin/maludb"
install -m 0644 README.md "$work_dir/$package/README.md"

if [ -f LICENSE ]; then
  install -m 0644 LICENSE "$work_dir/$package/LICENSE"
fi

cat > "$work_dir/$package/install.sh" <<'INSTALL'
#!/usr/bin/env bash
set -euo pipefail

prefix="${PREFIX:-/usr/local}"
install -d "$prefix/bin"
install -m 0755 "$(dirname "$0")/bin/maludb" "$prefix/bin/maludb"
echo "Installed maludb to $prefix/bin/maludb"
INSTALL
chmod 0755 "$work_dir/$package/install.sh"

tar -C "$work_dir" -czf "$dist_dir/$archive" "$package"
checksum_file "$dist_dir/$archive" "$dist_dir/$archive.sha256"

echo "Wrote $dist_dir/$archive"
echo "Wrote $dist_dir/$archive.sha256"
