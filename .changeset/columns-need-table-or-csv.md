---
"gws-rust": patch
---

`--columns` with the `json` or `yaml` output format (the JSON default included) is now an error (exit 3) instead of being silently ignored. It only shapes `table` and `csv` output; add `--format table` or `--format csv`, or select fields with `--jq`.
