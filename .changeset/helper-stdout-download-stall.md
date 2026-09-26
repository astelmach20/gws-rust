---
"gws-rust": patch
---

`--output -` on the download helpers (`drive +download`, `drive +export`) now honors the `--timeout`/`GWSR_TIMEOUT` idle limit and checks `Content-Length`, like a download to a file. A body that stalled mid-stream used to hang forever; it now fails as a network error (exit 10), and a dropped connection mid-stream is also reported as a network error instead of a generic one.
