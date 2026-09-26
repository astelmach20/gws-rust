---
"gws-rust": patch
---

`gwsr batch` now applies `--jq`, `--format` and `--columns` to its result lines, as it does for a `--page-all` stream. Before, the results were always printed as raw NDJSON and these options were silently ignored, although `--dry-run` did apply `--jq`. For example, `gwsr batch gmail --jq .body.snippet` printed the whole result objects instead of the snippets.
