---
"gws-rust": patch
---

Resumable uploads (`--upload` over 5 MiB or `--upload-resumable`, and helper uploads) that give up after their chunk retries now exit with the class of the last failure: a retryable status such as `503` or `429` exits `6` with the API's error message, and a lost connection or timeout exits `10`. Previously every exhausted upload exited `5` (internal error).
