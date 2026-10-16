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

# 3. Changesets must have front matter that `changeset version` accepts, naming the gws-rust
#    package and nothing else with a valid bump type. Anything this cannot read is an error: a
#    changeset that passes here but fails to parse breaks the release job on main.
# shellcheck disable=SC2016 # the single-quoted program is awk, not shell
frontmatter_awk='
  { sub(/\r$/, "") }
  state == 0 && /^[[:space:]]*$/ { next }
  state == 0 {
    if ($0 !~ /^---[[:space:]]*$/) exit
    state = 1; next
  }
  state == 1 && /^---[[:space:]]*$/ { state = 2; exit }
  state == 1 && (/^[[:space:]]*$/ || /^[[:space:]]*#/) { next }
  state == 1 {
    line = $0
    if (match(line, /^("[^"]+"|'\''[^'\'']+'\''|[^"'\'':#[:space:]][^:#]*)[[:space:]]*:[[:space:]]*/)) {
      key = substr(line, 1, RLENGTH); value = substr(line, RLENGTH + 1)
      sub(/[[:space:]]*:[[:space:]]*$/, "", key)
      sub(/[[:space:]]+(#.*)?$/, "", value)
      if (key ~ /^["'\'']/) key = substr(key, 2, length(key) - 2)
      if (value ~ /^"[^"]*"$/ || value ~ /^'\''[^'\'']*'\''$/) value = substr(value, 2, length(value) - 2)
      if (value ~ /^(major|minor|patch|none)$/) { print "package\t" key; next }
      print "error\tinvalid bump type \"" value "\" for \"" key "\"; use major, minor, patch or none"
      next
    }
    print "error\tunreadable front matter line: " line
  }
  END {
    if (state == 0) print "error\tmissing front matter: the file must start with a --- line"
    if (state == 1) print "error\tfront matter is not closed with a --- line"
  }
'
while IFS= read -r file; do
  [[ -z "$file" || "$file" == .changeset/README.md ]] && continue
  # Front matter lines look like:  "gws-rust": patch
  while IFS=$'\t' read -r kind detail; do
    if [[ "$kind" == error ]]; then
      echo "::error file=${file}::Invalid changeset: ${detail}."
      status=1
    elif [[ "$detail" != gws-rust ]]; then
      echo "::error file=${file}::Unknown package '${detail}' in changeset; the only package is \"gws-rust\"."
      status=1
    fi
  done < <(awk "$frontmatter_awk" "$file")
done < <(grep -E '^\.changeset/[^/]+\.md$' <<<"$changed" || true)

if [[ "$status" -eq 0 ]]; then
  echo "Policy checks passed."
fi
exit "$status"
