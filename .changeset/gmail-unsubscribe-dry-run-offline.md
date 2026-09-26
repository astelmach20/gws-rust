---
"gws-rust": patch
---

`gmail +unsubscribe --dry-run` now loads no credentials and sends nothing, as documented for `--dry-run`. Before, it loaded credentials and read the message's headers from Gmail, so it failed with an auth error (exit 2) when no account was signed in. The plan now lists the metadata read and the one-click POST, with a placeholder for the unsubscribe URL, which only the message itself can supply.
