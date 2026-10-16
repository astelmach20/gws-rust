---
"gws-rust": patch
---

When `gwsr batch` stops because a result line cannot be printed (a `--jq` or `--format` error, or a failed write to stdout), the error now names the calls that were already applied but whose results were not printed, and the calls that were never sent. Before, it reported only the `--jq` error (exit 3), as if nothing had been sent, so re-running the batch could repeat writes that had already been applied.
