---
"gws-rust": patch
---

Error hints now name settings that work for the command that failed:

- The network-error hint suggested raising `--timeout`, and the quota-project hints said to pass `--no-quota-project`. Only generated API methods have those flags, not helpers or `gwsr batch`. The hints now lead with `GWSR_TIMEOUT` / `timeout_secs` and `GWSR_NO_QUOTA_PROJECT=1`, and mention the flags as the per-method alternative.
- The rate-limit hint suggested "lower concurrency", a setting gwsr does not have. It now says to run fewer calls in parallel, or to raise `--page-delay` / `GWSR_PAGE_DELAY_MS` when paginating.
- The `events +subscribe` error for an operation that reports no subscription yet had a run of stray spaces in it, and named `gwsr events operations get` without the required `--params`. It now gives the full command.
