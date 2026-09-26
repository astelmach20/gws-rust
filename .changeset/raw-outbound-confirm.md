---
"gws-rust": patch
---

`GWSR_REQUIRE_CONFIRM=1` now also gates the generated methods that send mail or messages, share files or run scripts (`gmail users messages send`, `gmail users drafts send`, `chat spaces messages create`, `drive permissions create`/`update`, `script scripts run`, `admin members insert`), both directly and inside `gwsr batch`. Previously only the `+` helpers were gated, so the policy could be bypassed by calling the raw method. These methods now accept `-y/--yes`.
