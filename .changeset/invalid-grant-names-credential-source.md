---
"gws-rust": patch
---

A refresh token rejected by Google (`invalid_grant`) no longer always says "Run `gwsr auth login` to sign in again". For `GWSR_CREDENTIALS_FILE`, which takes precedence over every profile so a new login changes nothing, the error now names the file and says to replace it or unset the variable. For `GOOGLE_APPLICATION_CREDENTIALS` it names that file, and for gcloud's Application Default Credentials it suggests `gcloud auth application-default login`. `gwsr auth status` reports the same fix.
