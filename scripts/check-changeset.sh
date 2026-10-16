#!/usr/bin/env bash
# PR policy checks (run by CI on pull requests):
#   1. The CLI must not depend on generated google-* API crates (see AGENTS.md).
#   2. PRs that change Rust code or Cargo manifests must add a changeset.
#   3. Changesets may only name the "gws-rust" package.
#
#   scripts/check-changeset.sh <base-ref>     e.g. origin/main
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <base-ref>" >&2
  exit 2
fi
base="$1"

status=0

# 1. No generated google-* registry crates, in any form Cargo accepts: `google-x = ...`,
#    `google-x.workspace = true`, a `[dependencies.google-x]` table, a rename via
#    `package = "google-x"`, or a `[workspace.dependencies]` entry in the root manifest.
#    Inline path dependencies (our own crates) are fine. Prints the offending line numbers.
# shellcheck disable=SC2016 # the single-quoted program is awk, not shell
google_crates_awk='
  { sub(/\r$/, "") }
  /^[[:space:]]*\[/ {
    header = $0
    sub(/#.*/, "", header)
    gsub(/[[:space:]"'\'']/, "", header)
    in_deps = (header ~ /(^\[|\.)(dev-|build-)?dependencies\]$/)
    if (header ~ /(^\[|\.)(dev-|build-)?dependencies\.google-[A-Za-z0-9_-]+\]$/) print FNR
    next
  }
  in_deps && /^[[:space:]]*["'\'']?google-[A-Za-z0-9_-]+["'\'']?[[:space:]]*[.=]/ && !/path[[:space:]]*=/ { print FNR; next }
  /(^|[{,[:space:]])package[[:space:]]*=[[:space:]]*["'\'']google-/ { print FNR }
'
for manifest in Cargo.toml crates/*/Cargo.toml; do
  [[ -f "$manifest" ]] || continue
  offending="$(awk "$google_crates_awk" "$manifest")"
  for line in $offending; do
    echo "::error file=${manifest},line=${line}::Generated google-* crates are not allowed; the CLI builds its commands from Discovery documents at runtime."
    status=1
  done
done

merge_base="$(git merge-base "$base" HEAD)"
changed="$(git diff --name-only --diff-filter=ACMR "$merge_base" HEAD)"

# 2. Changeset required for Rust changes.
if grep -Eq '(\.rs$|(^|/)Cargo\.toml$|^Cargo\.lock$)' <<<"$changed"; then
  added_changesets="$(grep -E '^\.changeset/[^/]+\.md$' <<<"$changed" | grep -vc '^\.changeset/README\.md$' || true)"
  if [[ "$added_changesets" -eq 0 ]]; then
    echo "::error::This PR changes Rust code or Cargo manifests but adds no changeset. Run 'pnpm changeset' and commit the generated .changeset/*.md file."
    status=1
  fi
else
  echo "No Rust or Cargo changes; changeset not required."
fi

# 3. Changesets must name the gws-rust package and nothing else.
while IFS= read -r file; do
  [[ -z "$file" || "$file" == .changeset/README.md ]] && continue
  # Front matter lines look like:  "gws-rust": patch
  while IFS= read -r pkg; do
    if [[ "$pkg" != gws-rust ]]; then
      echo "::error file=${file}::Unknown package '${pkg}' in changeset; the only package is \"gws-rust\"."
      status=1
    fi
  done < <(awk '/^---$/ { n++; next } n == 1' "$file" | sed -nE 's/^["'\'']?([^"'\'':]+)["'\'']?:[[:space:]]*(major|minor|patch)[[:space:]]*$/\1/p')
done < <(grep -E '^\.changeset/[^/]+\.md$' <<<"$changed" || true)

if [[ "$status" -eq 0 ]]; then
  echo "Policy checks passed."
fi
exit "$status"
