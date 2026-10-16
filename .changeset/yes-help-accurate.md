---
"gws-rust": patch
---

The `--yes` help no longer says it is "required when not on a terminal". It now says what the confirmation gate does: when stdin or stderr is not a terminal, destructive actions require `--yes`, and outbound ones (send, share, notify, run code, such as `gmail +reply` or `admin +group-add-member`) require it only when `GWSR_REQUIRE_CONFIRM=1`. The generated skills, including the method-flag table in `gwsr-shared`, use the same text.
