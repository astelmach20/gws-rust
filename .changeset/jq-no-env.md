---
"gws-rust": patch
---

`--jq` no longer offers jq's `env` builtin. It returned the whole process environment, so a filter such as `--jq env.GWSR_TOKEN` printed the access token, client secret or any other secret variable to stdout, although gwsr never echoes them itself. A filter that uses `env` is now rejected before any request (exit 3).
