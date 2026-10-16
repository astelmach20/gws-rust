---
"gws-rust": patch
---

Recipe skills: Sheets ranges containing `!` are single-quoted (double quotes let interactive bash history-expand them), `docs +write` steps that write Markdown headings pass `--markdown` (they inserted literal `##` text), multi-line `docs +write` content is piped through `printf` so each step stays one line, and the weekly-schedule recipe uses `calendar +agenda --week` (plain `+agenda` shows one day).
