#!/usr/bin/env bash
# Changesets "version" command: bumps package.json via `changeset version`, then propagates the
# new version to every other place that carries it, refreshes Cargo.lock for the workspace
# crates only, regenerates skills, and stages the result for the release PR.
#
# Files updated:
#   Cargo.toml                      [workspace.package] version
#   crates/gws-rust/Cargo.toml      gws-rust-core dependency version
#   npm/package.json                version + optionalDependencies
#   gemini-extension.json           version
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

pnpm changeset version

version="$(node -p "require('./package.json').version")"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "error: package.json version '${version}' is not a semver version" >&2
  exit 1
fi
echo "Syncing version ${version}"

# Replace `version = "..."` only inside the [workspace.package] table.
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
awk -v ver="$version" '
  /^\[/ { in_table = ($0 == "[workspace.package]") }
  in_table && /^version[[:space:]]*=/ { $0 = "version = \"" ver "\""; replaced = 1 }
  { print }
  END { if (!replaced) exit 3 }
' Cargo.toml >"$tmp" || {
  echo "error: no version key found under [workspace.package] in Cargo.toml" >&2
  exit 1
}
cp "$tmp" Cargo.toml

# The CLI crate pins the core crate by version for crates.io; keep it identical.
core_dep_re='^(gws-rust-core = \{ version = ")[^"]+(".*)$'
if ! grep -Eq "$core_dep_re" crates/gws-rust/Cargo.toml; then
  echo "error: could not find the gws-rust-core dependency line in crates/gws-rust/Cargo.toml" >&2
  exit 1
fi
sed -E "s/${core_dep_re}/\\1${version}\\2/" crates/gws-rust/Cargo.toml >"$tmp"
cp "$tmp" crates/gws-rust/Cargo.toml

# shellcheck disable=SC2016 # the single-quoted program is JavaScript, not shell
VERSION="$version" node --input-type=module -e '
  import fs from "node:fs";
  const version = process.env.VERSION;
  const write = (file, data) => fs.writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`);

  const npmPkg = JSON.parse(fs.readFileSync("npm/package.json", "utf8"));
  const { platforms } = JSON.parse(fs.readFileSync("npm/platforms.json", "utf8"));
  npmPkg.version = version;
  npmPkg.optionalDependencies = Object.fromEntries(platforms.map((p) => [p.package, version]));
  write("npm/package.json", npmPkg);

  const ext = JSON.parse(fs.readFileSync("gemini-extension.json", "utf8"));
  ext.version = version;
  write("gemini-extension.json", ext);
'

# Only the workspace members change; do not float third-party dependencies in a release PR.
cargo update --workspace

# Skills embed the CLI version in their metadata.
cargo run --locked -- generate-skills --output-dir skills

git add Cargo.toml Cargo.lock crates/gws-rust/Cargo.toml npm/package.json gemini-extension.json skills/ docs/skills.md
echo "Version ${version} synced."
