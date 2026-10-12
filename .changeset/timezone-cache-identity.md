---
"gws-rust": patch
---

The cached account time zone is now kept per identity (profile, credentials file, ADC file and `--impersonate` user) and is never cached for `GWSR_TOKEN`/`GWSR_TOKEN_FILE`. Previously one global cache file was shared, so after switching `--profile`, `gwsr auth use`, `GWSR_CREDENTIALS_FILE` or the `--impersonate` subject, calendar and workflow helpers used the previous account's time zone for up to 24 hours (wrong "today" window for `+agenda`/`+standup-report`, wrong event times for `+insert` without an offset). Existing cache files from older versions are ignored and removed on the next `gwsr auth login`/`logout`.
