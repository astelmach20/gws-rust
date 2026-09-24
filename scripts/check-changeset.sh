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

# 1. No generated google-* registry crates. Path dependencies (our own crates) are fine.
for manifest in crates/*/Cargo.toml; do
  if offending="$(grep -En '^google-[A-Za-z0-9_-]+[[:space:]]*=' "$manifest" | grep -v 'path[[:space:]]*=')"; then
    while IFS= read -r line; do
      echo "::error file=${manifest},line=${line%%:*}::Generated google-* crates are not allowed; the CLI builds its commands from Discovery documents at runtime."
    done <<<"$offending"
    status=1
  fi
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
