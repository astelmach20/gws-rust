---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` now refuse `--jq`, `--columns` and a non-JSON `--format` together with `--output-dir` (exit 3). Before, these flags were silently ignored: `--output-dir` writes each record as JSON to its own file and prints nothing on stdout. A `format` default from the environment or `config.toml` is not affected.
