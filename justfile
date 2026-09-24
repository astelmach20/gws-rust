# Developer task runner (https://just.systems). `just` lists recipes.
# Every recipe mirrors a CI job, so `just ci` locally predicts a green PR.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List recipes
default:
    @just --list

# Build all targets
build:
    cargo build --workspace --all-targets --all-features --locked

# Build the optimized gwsr binary (target/release/gwsr)
release:
    cargo build --release --locked --bin gwsr

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Format all Rust code
fmt:
    cargo fmt --all

# Clippy with every target and feature, warnings as errors
clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Formatting, clippy, unused dependencies, shell scripts and workflows
lint: fmt-check clippy machete shellcheck workflows

# Rust tests
test:
    cargo test --workspace --all-targets --all-features --locked

# Tests for the npm shim and release scripts
test-js:
    node --test "npm/test/*.test.mjs" "scripts/test/*.test.mjs"

# Coverage report with the enforced line-coverage floor (pass --open to view it)
coverage *args:
    scripts/coverage.sh {{args}}

# Licenses, bans, sources and advisories (cargo-deny) plus RustSec audit
deny:
    cargo deny --all-features --locked check
    cargo audit --deny warnings

# Unused dependencies
machete:
    cargo machete

# Lint shell scripts
shellcheck:
    shellcheck --severity=style scripts/*.sh

# Lint GitHub Actions workflows
workflows:
    actionlint
    zizmor --pedantic .github

# Regenerate agent skills from the live Discovery documents (network access required)
skills:
    cargo run --locked -- dev generate-skills --output-dir skills --index docs/skills.md

# Everything CI checks, in roughly the same order
ci: lint test test-js deny
