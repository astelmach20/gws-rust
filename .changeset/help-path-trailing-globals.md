---
"gws-rust": patch
---

`gwsr help <service> ...` no longer fails with "unrecognized subcommand" when a global option such as `--format`, `--pretty` or `-v` follows the command path (`gwsr help drive files list --format table`).
