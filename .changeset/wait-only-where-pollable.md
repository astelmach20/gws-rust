---
"gws-rust": patch
---

`--wait` and `--wait-timeout` now appear only on methods whose operation the API's `operations.get` can poll. Before, any method returning a schema named `*Operation` offered them: on Compute Engine and other APIs without `operations.get`, `--wait` always failed, and `-o` tried to wait after the request was already sent. On Cloud DNS `managedZones.patch`, `--wait` was silently ignored.
