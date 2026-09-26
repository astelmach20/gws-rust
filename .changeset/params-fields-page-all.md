---
"gws-rust": patch
---

`--page-all` (and `--page-items`) now adds `nextPageToken` to a `fields` mask passed in `--params`, the same way `--fields` already did. Before this change, `--params '{"fields":"files(id)"}' --page-all` stopped after the first page because the API left out the token, and the command still exited 0.
