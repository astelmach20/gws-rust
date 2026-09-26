---
"gws-rust": patch
---

Breaking: `admin +user-suspend` no longer has a `--reason` flag. The reason was sent as the user's `suspensionReason`, which the Directory API marks output-only and ignores, so the account never recorded it, even though the help said it did. Passing `--reason` is now a usage error instead of being silently dropped.
