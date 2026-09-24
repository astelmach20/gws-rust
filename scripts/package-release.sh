#!/usr/bin/env bash
# Packages one release binary into a deterministic archive.
#
#   scripts/package-release.sh <version> <target> <binary> <out-dir>
#
# Produces <out-dir>/gws-rust-<version>-<target>.tar.gz (or .zip for Windows targets) containing
# a single top-level directory gws-rust-<version>-<target>/ with the binary, LICENSE, NOTICE,
# README.md and CHANGELOG.md. The archive is byte-for-byte reproducible for the same inputs:
# entries are sorted, owners are 0:0, and every mtime is SOURCE_DATE_EPOCH (required).
#
# Requires GNU tar (set TAR=gtar on macOS) and, for Windows targets, Info-ZIP `zip`.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <version> <target> <binary> <out-dir>" >&2
  exit 2
fi
version="$1"
target="$2"
binary="$3"
out_dir="$4"

if [[ -z "${SOURCE_DATE_EPOCH:-}" || ! "$SOURCE_DATE_EPOCH" =~ ^[0-9]+$ ]]; then
  echo "error: SOURCE_DATE_EPOCH must be set to a Unix timestamp (e.g. the commit time)" >&2
  exit 1
fi
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "error: version must be semver without a leading 'v': ${version}" >&2
  exit 1
fi
if [[ ! -f "$binary" ]]; then
  echo "error: binary not found: ${binary}" >&2
  exit 1
fi

if ! touch --version 2>/dev/null | grep -q GNU; then
  echo "error: GNU coreutils touch is required (install coreutils; on macOS put its gnubin first in PATH)" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
stem="gws-rust-${version}-${target}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir "${staging}/${stem}"

case "$target" in
  *windows*) bin_name=gwsr.exe ;;
  *) bin_name=gwsr ;;
esac
install -m 0755 "$binary" "${staging}/${stem}/${bin_name}"
for doc in LICENSE NOTICE README.md CHANGELOG.md; do
  install -m 0644 "${repo_root}/${doc}" "${staging}/${stem}/${doc}"
done

# Normalize mtimes so neither tar nor zip records build-time timestamps.
find "${staging}/${stem}" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

case "$target" in
  *windows*)
    archive="${out_dir}/${stem}.zip"
    rm -f "$archive"
    # -X drops uid/gid and extended timestamps; -D omits directory entries; TZ=UTC pins DOS times.
    (cd "$staging" && find "$stem" -type f | LC_ALL=C sort | TZ=UTC zip -X -D -q -9 "$archive" -@)
    ;;
  *)
    tar_bin="${TAR:-tar}"
    if ! "$tar_bin" --version 2>/dev/null | grep -q 'GNU tar'; then
      echo "error: GNU tar is required for reproducible archives (install gnu-tar and set TAR=gtar)" >&2
      exit 1
    fi
    archive="${out_dir}/${stem}.tar.gz"
    "$tar_bin" --create --format=gnu --sort=name \
      --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner \
      --mode='u+rwX,go+rX,go-w' \
      --directory "$staging" "$stem" |
      gzip -9 --no-name >"$archive"
    ;;
esac

echo "$archive"
