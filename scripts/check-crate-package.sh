#!/usr/bin/env bash
# Checks that every published crate ships LICENSE and NOTICE, as Apache-2.0 sections 4(a) and
# 4(d) require for redistributions. Lists each package's files with `cargo package --list`
# (no build) and fails if either file is missing.
#
#   scripts/check-crate-package.sh
set -euo pipefail

cd "$(dirname "$0")/.."

status=0
for crate in gws-rust gws-rust-core; do
  files="$(cargo package --list --package "$crate" --allow-dirty --locked)"
  for required in LICENSE NOTICE; do
    if ! grep -qxF "$required" <<<"$files"; then
      echo "error: the $crate crate package is missing $required" >&2
      status=1
    fi
  done
done

if [[ $status -eq 0 ]]; then
  echo "LICENSE and NOTICE are in every crate package"
fi
exit "$status"
