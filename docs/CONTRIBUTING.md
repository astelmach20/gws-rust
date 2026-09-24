# Contributing

Thanks for helping improve `gwsr`. Everyone who takes part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

Before you write code, read [AGENTS.md](../AGENTS.md). It has the architecture, the project rules (fail loudly, no compatibility shims, JSON by default, tests for every behavior change) and the input-validation checklist that reviews enforce.

## Setup

```bash
git clone https://github.com/astelmach20/gws-rust && cd gws-rust
nix develop      # optional: the pinned Rust toolchain and every tool below
pnpm install     # changesets CLI and lefthook (pnpm 11.27.1, pinned in package.json)
pnpm run hooks   # optional: install the lefthook git hooks (primary checkout only)
just             # list recipes
```

Without Nix, install [rustup](https://rustup.rs) (it picks up `rust-toolchain.toml`), [just](https://just.systems), pnpm and, as needed, `cargo-deny`, `cargo-audit`, `cargo-llvm-cov`, `cargo-machete`, `cargo-insta`, `shellcheck`, `actionlint` and `zizmor`.

## Making a change

1. Branch from `main`.
2. Make the change, with focused tests. Validation logic needs a test for both the accept path and the reject path.
3. Run `just ci`: fmt, clippy, machete, shellcheck, actionlint/zizmor, the Rust and npm tests, and `cargo deny` + `cargo audit`. If you installed the hooks (`pnpm run hooks`), they run the fast subset on commit and push.
4. Run `just coverage` if you touched a lot of code. CI fails when line coverage falls below the floor in `scripts/coverage.sh`.
5. If you changed help text, helper flags, `registry/*.toml` or the skill generator, run `just skills` and commit the regenerated `skills/` and `docs/skills.md`, including new and deleted skill directories. Never edit a generated `SKILL.md` by hand. A full run deletes generated skills that are no longer produced; a directory reported as `unmanaged` has no generator marker, so delete it by hand if it is stale.
6. If CLI output or help changed, update the snapshots: `cargo insta review` (or `INSTA_UPDATE=always cargo test`).
7. Add a changeset (below) and open a pull request. All changes, including maintainers', go through review.

### Changesets

Every PR that changes Rust code or a Cargo manifest needs a changeset. Run `pnpm changeset`, or create `.changeset/<descriptive-name>.md`:

```markdown
---
"gws-rust": patch
---

What changed, from the user's point of view.
```

Before 1.0, use `minor` for new features and breaking changes (and say which parts are breaking), and `patch` for fixes. `gws-rust` is the only package name CI accepts.

### Testing notes

- Never use real Google accounts or credentials in tests. Mock HTTP with `wiremock`, and use the stripped Discovery fixtures in `crates/gws-rust/tests/fixtures/discovery/`.
- Tests that change the process working directory or environment must be `#[serial]` (`serial_test`).
- Canonicalize temp-dir paths before comparing them (macOS `/var` is a symlink to `/private/var`).

## Live API smoketest (maintainers)

`.github/workflows/smoketest.yml` runs a release build against the live APIs on pushes to `main` and nightly. It is opt-in: it runs only when the repository variable `GWSR_SMOKETEST` is `true`, and it reads the `GWSR_SMOKETEST_CREDENTIALS` secret from the `smoketest` environment. Use a dedicated test account, never a personal one.

To create or rotate the credential:

```bash
export GWSR_CONFIG_DIR="$(mktemp -d)"          # keep it out of your own profile
cargo run -- auth login --scopes drive.readonly,gmail.readonly,calendar.readonly
cargo run -- auth export --unmasked --output smoketest-creds.json
base64 < smoketest-creds.json | gh secret set GWSR_SMOKETEST_CREDENTIALS --env smoketest
rm smoketest-creds.json && cargo run -- auth logout --no-revoke && rm -rf "$GWSR_CONFIG_DIR"
gh variable set GWSR_SMOKETEST --body true
```

## Reporting security issues

Do not open a public issue. Follow [SECURITY.md](../SECURITY.md).
