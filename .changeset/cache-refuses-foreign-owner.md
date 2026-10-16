---
"gws-rust": patch
---

Discovery documents and account time zones are no longer read from a cache directory or file that belongs to another user. Group- or world-writable cache files were already refused, but a cache that another user created with an ordinary mode such as `0755`/`0644` was trusted. That can happen under a shared directory such as `GWSR_CACHE_DIR=/tmp/gwsr`. A planted Discovery document could remap a command to a different HTTP method and path, which then ran with your credentials. Such entries are now ignored and fetched again.
