---
"gws-rust": patch
---

A 403 caused by the quota project sent as `x-goog-user-project` (the caller lacks `serviceusage.services.use` on it, for example because the OAuth client belongs to a project the user is not a member of) now carries a hint pointing at `--no-quota-project` / `GWSR_NO_QUOTA_PROJECT=1` and `GWSR_PROJECT_ID`.
