---
"gws-rust": patch
---
`script +run` classifies a failed run by the code of its error status: an exception thrown by the script now exits 1 (HTTP 400, permanent) instead of 6 (retryable), and a run that hit its deadline (DEADLINE_EXCEEDED) still exits 6.
