---
"gws-rust": patch
---

The `gwsr-docs-write` skill now shows that `docs +write` needs `--text` or `--text-file`. Its usage line was `gwsr docs +write --document-id <ID>`, which clap rejects, and the flags table marked both content flags as not required. A new test checks every committed helper skill against the helpers' own definitions, because the CI drift check only warns.
