---
"gws-rust": patch
---

`GWSR_API_BASE_URL` now applies to the `workflow` helpers and to the account time zone lookup used by calendar and workflow helpers. Before, these always sent their requests (with the bearer token) to the public Google hosts, bypassing the configured private endpoint or recording proxy.
