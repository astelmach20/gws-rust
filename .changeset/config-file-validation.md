---
"gws-rust": patch
---

Every `config.toml` value is now validated when the file is loaded, whether or not an environment variable or flag overrides it. An invalid value is a configuration error (exit 8) that names the file. Before this fix:

- an invalid `format`, `json_style` or `sanitize_mode` loaded without error while the matching `GWSR_*` variable was set.
- an invalid `sanitize_template` was only caught when a service command used it, and was then reported as a validation error (exit 3).
- `log_file` accepted a relative path, so the log directory was created under whatever directory `gwsr` ran in. Like `GWSR_LOG_FILE`, it must now be absolute.
