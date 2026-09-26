---
"gws-rust": patch
---

`gwsr auth login` and `gwsr auth logout` now clear the access-token cache while holding its cross-process lock. Previously a concurrent `gwsr` process that was refreshing a token could write the previous grant's access token back after the cache was cleared, so a later command could use a token for the old account or scope set. `auth logout` now also lists the cache lock file in `removed`.
