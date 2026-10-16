---
"gws-rust": patch
---

The cached account time zone is now kept per credentials file even when `GWSR_CREDENTIALS_FILE` or `GOOGLE_APPLICATION_CREDENTIALS` is a relative path or a symlink. The cache key used the path as written, so `./sa.json` in two project directories (two different accounts) shared one cached time zone for up to 24 hours.
