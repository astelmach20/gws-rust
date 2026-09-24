#!/usr/bin/env bash
# Publishes the packages produced by npm/scripts/build-packages.mjs, in publish-order.txt order
# (platform packages first, so the main package never references a missing version).
#
#   scripts/npm-publish.sh <packages-dir> <dist-tag>
#
# Authentication is npm trusted publishing (OIDC) from GitHub Actions; no NPM_TOKEN is used.
# Re-running after a partial failure is safe: versions that already exist are skipped explicitly.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <packages-dir> <dist-tag>" >&2
  exit 2
fi
packages_dir="$1"
dist_tag="$2"
order_file="${packages_dir}/publish-order.txt"

if [[ ! -r "$order_file" ]]; then
  echo "error: ${order_file} not found; run npm/scripts/build-packages.mjs first" >&2
  exit 1
fi

registry="$(npm config get registry)"
if [[ "$registry" != "https://registry.npmjs.org/" ]]; then
  echo "error: refusing to publish to ${registry}; expected https://registry.npmjs.org/" >&2
  exit 1
fi

while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  dir="${packages_dir}/${name}"
  version="$(node -p "require(process.argv[1]).version" "$(cd "$dir" && pwd)/package.json")"
  if npm view "${name}@${version}" version >/dev/null 2>&1; then
    echo "::notice::${name}@${version} is already published; skipping."
    continue
  fi
  echo "Publishing ${name}@${version} (dist-tag ${dist_tag})"
  npm publish "$dir" --access public --provenance --tag "$dist_tag"
done <"$order_file"
