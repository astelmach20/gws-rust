---
"gws-rust": patch
---
On Windows, gcloud's Application Default Credentials are found in `%APPDATA%\gcloud\application_default_credentials.json`, where `gcloud auth application-default login` writes them, instead of a `~/.config/gcloud` path gcloud never uses there.
