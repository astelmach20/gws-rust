---
"gws-rust": patch
---

The config directory, `config.toml` and `profile.json` are no longer read when another user owns them or when group or others may write to them. This matters under a shared location such as `GWSR_CONFIG_DIR=/tmp/gwsr`, where another user could create the directory first and plant a config that picks the profile, log file or Model Armor template. Such a directory or file is now a configuration error (exit 8) that names the path and the `chmod`/`chown` fix. Credential, token, client and key files, which were already refused when group or others could read them, are now also refused when another user owns them. Files owned by root are still accepted.
