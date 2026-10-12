---
"gws-rust": patch
---

`events +renew --all --reactivate` now reactivates every SUSPENDED subscription for the given event types. It used to pick subscriptions by expiry time, so it called `:reactivate` on active subscriptions (which the API rejects, failing the command) and skipped suspended ones that were not about to expire. `--within` applies only to renewals.
