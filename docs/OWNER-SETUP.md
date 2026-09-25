# Owner setup checklist

One-time steps that can't be done from inside the repository. Do them in order, before tagging the
first release. Run the commands on a machine where `gh` is logged in as `astelmach20`:

```bash
gh auth status
```

## 0. Delete the stray upstream fork

`andrew-stelmach-fleet/googleworkspace-cli` is an unused fork of the upstream CLI. It belongs to the
Fleet account, so this step needs that account, not `astelmach20`. Skip it if the repo is already
gone.

```bash
gh auth refresh -h github.com -s delete_repo
gh repo delete andrew-stelmach-fleet/googleworkspace-cli --yes
```

## 1. Reserve the package names

Until you own these names, anyone can publish under them. Both registries only allow trusted
publishing on a package that already exists, so publish an empty `0.0.0` placeholder under each
name once. Protect both registry accounts with 2FA before you start.

### crates.io

1. Sign in at <https://crates.io> with GitHub (`astelmach20`) and verify your email under Account
   Settings.
2. Create an API token under Account Settings → API Tokens with the `publish-new` scope and a
   1-day expiry.
3. Log in with the token, then publish the placeholders:

   ```bash
   cargo login
   ```

   ```bash
   for n in gws-rust gws-rust-core gwsr; do d=$(mktemp -d); mkdir "$d/src"; : > "$d/src/lib.rs"; printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\ndescription = "Name reserved for https://github.com/astelmach20/gws-rust"\nlicense = "Apache-2.0"\nrepository = "https://github.com/astelmach20/gws-rust"\n' "$n" > "$d/Cargo.toml"; (cd "$d" && cargo publish --allow-dirty) || break; done
   ```

4. Revoke the token, and log out so no token stays on disk:

   ```bash
   cargo logout
   ```

### npm

1. Turn on 2FA for your npm account (npmjs.com → Account → Two-Factor Authentication).
2. Log in and publish the placeholders. None of them has an install script.

   ```bash
   npm login
   ```

   ```bash
   for n in gws-rust gws-rust-darwin-arm64 gws-rust-darwin-x64 gws-rust-linux-arm64 gws-rust-linux-x64 gws-rust-win32-x64 gwsr; do d=$(mktemp -d); printf '{"name":"%s","version":"0.0.0","description":"Name reserved for https://github.com/astelmach20/gws-rust","license":"Apache-2.0","repository":{"type":"git","url":"git+https://github.com/astelmach20/gws-rust.git"}}\n' "$n" > "$d/package.json"; (cd "$d" && npm publish --access public) || break; done
   ```

npm may reject a name that looks too much like an existing package. `gwsr` is optional, so drop it
from the loop if that happens.

## 2. Trusted publishing

Set up a trusted publisher on each crate and each npm package from step 1:

- **crates.io:** open each crate → Settings → Trusted Publishing → Add → GitHub.
- **npm:** open each package → Settings → Trusted Publisher → GitHub Actions. On the same page,
  under Publishing access, choose "Require two-factor authentication and disallow tokens".

| Field | Value |
|---|---|
| Owner | `astelmach20` |
| Repository | `gws-rust` |
| Workflow | `release.yml` |
| Environment | `release` |

After this, the release workflow publishes without stored tokens. Delete any `CARGO_REGISTRY_TOKEN`
or `NPM_TOKEN` repository secrets if they exist.

## 3. The `release` environment

This command makes you the required reviewer for every release and allows deployments only from
`v*` tags. `58709763` is the user id of `astelmach20`.

```bash
gh api -X PUT repos/astelmach20/gws-rust/environments/release --input - <<'EOF'
{"reviewers":[{"type":"User","id":58709763}],"prevent_self_review":false,"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}
EOF
```

```bash
gh api -X POST repos/astelmach20/gws-rust/environments/release/deployment-branch-policies -f name='v*' -f type=tag
```

## 4. Repository security features

This turns on:

- secret scanning with push protection;
- private vulnerability reporting (`SECURITY.md` points reporters at it);
- Dependabot alerts and security updates.

```bash
gh api -X PATCH repos/astelmach20/gws-rust --input - <<'EOF'
{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}
EOF
```

```bash
gh api -X PUT repos/astelmach20/gws-rust/private-vulnerability-reporting
gh api -X PUT repos/astelmach20/gws-rust/vulnerability-alerts
gh api -X PUT repos/astelmach20/gws-rust/automated-security-fixes
```

## 5. Rulesets for `main` and release tags

The `main` ruleset:

- requires a pull request with code-owner review;
- requires the checks `CI OK`, `cargo-deny`, `cargo-audit`, `actionlint` and `zizmor`;
- blocks force pushes and deletion.

You are the only maintainer and can't approve your own PRs, so repository admins may bypass the
rule, but only through a pull request. Required checks still apply to the PR itself.

```bash
gh api -X POST repos/astelmach20/gws-rust/rulesets --input - <<'EOF'
{"name":"main","target":"branch","enforcement":"active",
 "conditions":{"ref_name":{"include":["~DEFAULT_BRANCH"],"exclude":[]}},
 "bypass_actors":[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"pull_request"}],
 "rules":[
  {"type":"deletion"},
  {"type":"non_fast_forward"},
  {"type":"pull_request","parameters":{"required_approving_review_count":1,"require_code_owner_review":true,"dismiss_stale_reviews_on_push":true,"require_last_push_approval":false,"required_review_thread_resolution":true}},
  {"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":false,"required_status_checks":[{"context":"CI OK"},{"context":"cargo-deny"},{"context":"cargo-audit"},{"context":"actionlint"},{"context":"zizmor"}]}}
 ]}
EOF
```

Tags trigger the release, so only admins may create, move or delete `v*` tags:

```bash
gh api -X POST repos/astelmach20/gws-rust/rulesets --input - <<'EOF'
{"name":"release tags","target":"tag","enforcement":"active",
 "conditions":{"ref_name":{"include":["refs/tags/v*"],"exclude":[]}},
 "bypass_actors":[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}],
 "rules":[{"type":"creation"},{"type":"update"},{"type":"deletion"}]}
EOF
```

Confirm that both rulesets exist:

```bash
gh api repos/astelmach20/gws-rust/rulesets --jq '.[] | "\(.name): \(.enforcement)"'
```

## 6. Optional: Homebrew tap

Without the tap, the release job skips it with a warning, and `brew install astelmach20/tap/gws-rust`
won't work.

1. Create the repository:

   ```bash
   gh repo create astelmach20/homebrew-tap --public --description "Homebrew formulae for gws-rust"
   ```

2. Create a fine-grained token at GitHub → Settings → Developer settings → Fine-grained tokens:
   - Repository access: only `astelmach20/homebrew-tap`.
   - Permission: Contents read and write.
3. Store it as a secret in the `release` environment. The command prompts for the value, so the
   token never lands in shell history:

   ```bash
   gh secret set HOMEBREW_TAP_TOKEN -R astelmach20/gws-rust --env release
   ```

## 7. Optional: bot app for automation PRs

PRs opened with `GITHUB_TOKEN` don't trigger CI. That affects the changesets "chore: release versions" PR
and the daily skills sync. To fix it:

1. Create a GitHub App at GitHub → Settings → Developer settings → GitHub Apps:
   - No webhook.
   - Repository permissions: Contents read and write, Pull requests read and write.
2. Install the app on `astelmach20/gws-rust` only, and generate a private key.
3. Save the app id and the private key:

   ```bash
   gh variable set BOT_APP_ID -R astelmach20/gws-rust --body "<app id>"
   ```

   ```bash
   gh secret set BOT_APP_PRIVATE_KEY -R astelmach20/gws-rust < path/to/private-key.pem
   ```

4. Delete the downloaded `.pem` afterwards.

## 8. First release

1. Merge the "chore: release versions" PR that changesets opens. It turns the pending changeset into the
   `0.23.0` version bump and CHANGELOG entry.
2. Tag the release with `scripts/tag-release.sh`, approve the `release` environment deployment, and
   watch the crates.io, npm and Homebrew jobs.
3. Verify one archive as the README describes (`gh attestation verify` and `cosign verify-blob`).
   Then install from each channel on a clean machine:

   ```bash
   npm install -g gws-rust && gwsr --version
   ```

   ```bash
   cargo install gws-rust --locked && gwsr --version
   ```

4. Re-record `docs/demo.gif` with `vhs docs/demo.tape`, using a throwaway Google account. The
   current recording still shows the upstream `gws` CLI.
