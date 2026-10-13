---
"gws-rust": patch
---

`gwsr auth status` reads the OAuth client from the config directory it reports, and unit tests no longer read or write the real config, cache, keyring or gcloud credentials.
