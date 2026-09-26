---
"gws-rust": patch
---

`gwsr auth setup` now validates the project ID from `--project`, from the wizard's "Enter a project ID" and "Create a new project" inputs, and from the current gcloud configuration. Before, any string was accepted. It was passed to `gcloud` as an argument, where a value such as `-q` or `--impersonate-service-account=...` is read as a flag. It was also put unescaped into the path of the consent-screen request sent with your gcloud access token, and saved as the quota project in the OAuth client file.
