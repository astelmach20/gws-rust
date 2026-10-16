---
"gws-rust": patch
---
`admin +audit` keeps the fractional seconds of `--since` and `--until` instead of truncating them, so events in the last sub-second of the range are no longer dropped.
