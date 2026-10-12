---
"gws-rust": patch
---

`alt=media` downloads with `-o PATH` or `-o -` now deliver the file's exact bytes even when it is JSON. Previously a stored JSON file was parsed and re-serialized (keys reordered, numbers and whitespace rewritten). A JSON file with `name` and `metadata` keys was polled as a long-running operation, and a file that was not one valid JSON document (for example NDJSON) failed to download.
