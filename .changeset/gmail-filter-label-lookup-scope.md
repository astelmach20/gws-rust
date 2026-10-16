---
"gws-rust": patch
---

`gmail +filter create` with `--add-label`, `--remove-label` or a label flag such as `--archive` or `--star` no longer fails with "insufficient scopes" when the profile was granted `gmail.settings.basic` (for example by `gwsr auth login --full`). The labels used to resolve label names are now listed with their own token (`gmail.readonly`, or every granted scope when that is not in the grant), because `users.labels.list` does not accept the token narrowed to `gmail.settings.basic` that creates the filter.
