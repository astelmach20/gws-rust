---
"gws-rust": patch
---

Discovery document fetches honor `GWSR_TIMEOUT` and `timeout_secs` in `config.toml`. They used a fixed 60-second limit, so a shorter timeout did not apply to them and `0` did not disable it. The timeout bounds the wait for the response and each gap while reading the document, as it does for API requests.
