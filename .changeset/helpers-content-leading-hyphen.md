---
"gws-rust": patch
---

`docs +write`/`+create --text`, `docs +replace --find` and `sheets +append`/`+write --values` accept a value that starts with `-`. Before, a Markdown list (`--text '- item'`), a rule (`---`), a negative number (`--values '-5,abc'`) or a search for `-5` was rejected as an unknown flag.
