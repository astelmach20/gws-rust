---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` no longer retry a permanent `--sanitize` failure (for example a Model Armor 400, 403 or 404, or an invalid template) as transient until `--max-failures` runs out. The stream now stops on the first such error with its own exit code; network and retryable API failures are still retried.
