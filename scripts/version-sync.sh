#!/usr/bin/env bash
# Syncs the version from package.json into all workspace Cargo.toml files,
# updates Cargo.lock, and regenerates skills.
# Used by changesets/action as a custom version command.
set -euo pipefail

# Run the standard changeset version command first
pnpm changeset version

# Read the new version from package.json
VERSION=$(node -p "require('./package.json').version")

# Update version in all workspace crate Cargo.toml files
# Uses awk to only change the version under [package], not other sections
for cargo_toml in crates/*/Cargo.toml; do
  tmp=$(mktemp)
  awk -v ver="$VERSION" '
    /^\[package\]/ { in_pkg=1 }
    /^\[/ && !/^\[package\]/ { in_pkg=0 }
    in_pkg && /^version = / { $0 = "version = \"" ver "\"" }
    { print }
  ' "$cargo_toml" > "$tmp" && mv "$tmp" "$cargo_toml"
done

# Update Cargo.lock to match
cargo generate-lockfile

# Update flake.lock if nix is available
if command -v nix > /dev/null 2>&1; then
  nix flake lock --update-input nixpkgs
fi

# Regenerate skills so metadata.version tracks the CLI version
cargo run -- generate-skills --output-dir skills

# Stage the changed files so changesets/action commits them
git add crates/*/Cargo.toml Cargo.lock flake.nix flake.lock skills/

