---
"gws-rust": patch
---

`gwsr auth login` now requests exactly the scopes you chose:

- `--services` no longer keeps `cloud-platform` (for example with `--full -s gmail`), and the scope picker no longer lists it as belonging to every service. Request it with `--scopes cloud-platform` or plain `--full` if you need it.
- The picker's "Full access" preset no longer adds `cloud-platform` when it is not a listed, checked row.
- When `--services` names a service the picker has no scopes for (for example `-s chat` without `gwsr auth setup`), the picker is skipped and that service's scopes come from its Discovery document instead of being dropped.
- An unknown `--services` name (such as `calendar.events`), or a service whose scopes cannot be looked up, is now an error instead of a warning.
