---
"gws-rust": patch
---

`gmail +filter create --forward` is now an outbound action: with `GWSR_REQUIRE_CONFIRM=1` it needs `--yes` (or a prompt on a terminal), like `gmail +forward`. Previously a filter that automatically forwards matching mail was created without confirmation. `+filter create` now accepts `-y/--yes`.
