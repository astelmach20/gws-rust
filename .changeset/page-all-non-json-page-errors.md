---
"gws-rust": patch
---

`--page-all` / `--page-items`: a later page that comes back as something other than JSON (for example an HTML proxy or captive-portal page served with status 200) now fails the run with exit `5` and names the `pageToken` to resume from. Previously the page was wrapped as a `{"status":"success",...}` object, mixed into the NDJSON stream, and the truncated listing exited `0` as if it were complete.
