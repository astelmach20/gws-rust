# Changesets

Every PR that changes Rust code needs a changeset describing the user-visible change
(CI enforces this; see `scripts/check-changeset.sh`). Create one with:

```sh
pnpm changeset
```

The only package name is `"gws-rust"`. On merge to `main`, the Release (Changeset) workflow
opens a "chore: release versions" PR; merging that PR tags `vX.Y.Z`, which runs the release
workflow. See the [changesets docs](https://github.com/changesets/changesets) for details.
