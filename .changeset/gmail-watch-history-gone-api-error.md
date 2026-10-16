---
"gws-rust": patch
---
When Gmail history since `gmail +watch`'s last historyId is no longer available, the command now exits with the API error (exit 1, keeping Google's reason and message, plus a note to restart `+watch`) instead of an internal error (exit 5).
