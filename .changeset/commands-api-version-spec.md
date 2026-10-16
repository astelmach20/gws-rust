---
"gws-rust": patch
---

`gwsr commands` accepts `<api>:<version>` like `schema`, `batch` and normal commands: `gwsr commands drive:v2` lists Drive v2 and `gwsr commands youtube:v3` lists an API that is not in the registry (under its API name, with the Discovery document's description). Both used to fail with "unknown service". An unknown name now gets the same error as elsewhere, with a "did you mean" suggestion.
