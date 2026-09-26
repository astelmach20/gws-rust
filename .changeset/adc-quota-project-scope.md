---
"gws-rust": patch
---

The `quota_project_id` from gcloud Application Default Credentials is now sent as `x-goog-user-project` only when ADC is the credential in use. With a `gwsr` profile, `GWSR_CREDENTIALS_FILE` or `GWSR_TOKEN`, and no `GWSR_PROJECT_ID` or client `project_id`, no quota project is sent instead of an unrelated gcloud project (which failed with a 403 naming that project).
