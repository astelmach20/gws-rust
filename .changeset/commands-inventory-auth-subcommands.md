---
"gws-rust": patch
---

`gwsr commands` now lists the `auth` subcommands (`auth login`, `auth status`, `auth setup`, ...) with their flags. It used to list a single `auth` command whose only flag was a raw `args` passthrough, so agents reading the inventory could not discover how to log in or check credentials.
