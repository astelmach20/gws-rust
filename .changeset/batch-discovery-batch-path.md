---
"gws-rust": patch
---

`gwsr batch` sends to the batch endpoint named by the API's Discovery `batchPath` (e.g. `https://sheets.googleapis.com/batch`) instead of a guessed `batch/<api>/<version>`, and refuses APIs that do not support batching.
