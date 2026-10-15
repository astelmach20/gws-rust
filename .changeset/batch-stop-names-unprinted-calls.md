---
"gws-rust": patch
---
When `gwsr batch` stops early (a later batch request fails, or `--sanitize` withholds a result), the error now names the calls that got no result and the calls that were never sent, and `--sanitize` no longer drops the other already-applied results in the same group.
