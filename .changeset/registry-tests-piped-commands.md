---
"gws-rust": patch
---

Tests only: the recipe and persona command check validates `gwsr` commands that a step pipes input into (`printf ... | gwsr docs +write --text-file -`) again. Since #442 these steps began with `printf`, so they were skipped. No behavior change.
