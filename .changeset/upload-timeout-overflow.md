---
"gws-rust": patch
---

Uploads (`--upload`) no longer crash when `--timeout` or `GWSR_TIMEOUT` is set to a huge value such as `18446744073709551615`. Adding the per-64-KiB upload allowance to the timeout overflowed. The timeout now saturates at the maximum.
