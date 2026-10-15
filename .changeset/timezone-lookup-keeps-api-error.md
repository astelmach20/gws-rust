---
"gws-rust": patch
---
When neither the account nor the local time zone can be determined, the account lookup's own error (for example a 403 missing-scope error, exit 1, with its scope hint) is now reported instead of being turned into a validation error (exit 3).
