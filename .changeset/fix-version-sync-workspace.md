---
"@googleworkspace/cli": patch
---

Fix version-sync script and bump CLI crate version to 0.21.0

The `version-sync.sh` script was updating the root `Cargo.toml` which no longer has a `[package]` section after the workspace refactor. Updated to target `crates/google-workspace-cli/Cargo.toml`. Also syncs the CLI crate version to 0.21.0 to match `package.json`.
