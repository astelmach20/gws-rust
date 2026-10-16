---
"gws-rust": patch
---

The generated `gwsr-workflow` and `gwsr-modelarmor` skills no longer tell agents to run `gwsr <service> <resource> <method>` or `gwsr schema <service>.<resource>.<method>`. Every command of these services is a `+helper`, so those commands failed; the skills now show `gwsr <service> +<helper> [flags]` and point at `gwsr <service> +<helper> --help`.
