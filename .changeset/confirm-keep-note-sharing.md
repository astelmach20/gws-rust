---
"gws-rust": patch
---

`keep notes permissions batchCreate`, which shares a Keep note with other people, is now gated as outbound like Drive's `permissions create`: with `GWSR_REQUIRE_CONFIRM=1` it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`. The command now takes `--yes`.
