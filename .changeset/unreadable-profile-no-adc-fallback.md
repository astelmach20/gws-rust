---
"gws-rust": patch
---

A profile directory that cannot be read (for example one left owned by root by `sudo gwsr auth login`) is now a credential-store error (exit 9). Before, gwsr treated it as "not logged in": the default profile silently fell back to Application Default Credentials, a different identity, and an explicit profile reported that it had no credentials. An unreadable `GWSR_CREDENTIALS_FILE`, `GOOGLE_APPLICATION_CREDENTIALS` or gcloud ADC path is likewise reported as such instead of as missing.
