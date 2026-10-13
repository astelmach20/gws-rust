---
"gws-rust": patch
---

`gwsr auth login` to the default profile no longer writes `profile = "default"` to `config.toml`. Writing it made the default profile count as explicitly chosen, so after `gwsr auth logout` every command failed with "profile 'default' has no credentials" instead of falling back to Application Default Credentials (`GOOGLE_APPLICATION_CREDENTIALS` or gcloud's ADC file), and every re-login hint added `--profile default`. A first login to a named profile still makes it the active one. An existing `profile = "default"` line written by an earlier version can be removed by hand.
