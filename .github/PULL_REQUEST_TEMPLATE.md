## What and why

<!-- What does this change, and which issue or audit item does it address? -->

## How it was tested

<!-- Commands run, new tests added. For a new command or flag, paste `--dry-run` output showing
the request matches the Discovery document. Never paste real tokens or personal data. -->

## Checklist

- [ ] `just ci` passes locally (fmt, clippy `--all-targets --all-features`, tests, cargo-deny, cargo-audit).
- [ ] New behavior has focused tests (unit, or integration with wiremock/assert_cmd).
- [ ] Errors are surfaced with context; nothing fails silently.
- [ ] No generated `google-*` API crates were added (the CLI builds commands from Discovery at runtime).
- [ ] A changeset is included (`pnpm changeset`) if Rust code or Cargo manifests changed.
