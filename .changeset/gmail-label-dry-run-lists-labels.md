---
"gws-rust": patch
---

`gmail +label --dry-run` and `gmail +filter create --dry-run` now list the `users.labels.list` read (`GET .../users/me/labels`) that a real run sends first to resolve label names to IDs. `+filter create` lists it only when the filter adds or removes labels (`--add-label`, `--remove-label`, `--archive`, `--star` and the like), as a real run does. The dry run still sends no request.
