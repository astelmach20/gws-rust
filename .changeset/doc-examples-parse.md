---
"gws-rust": patch
---

Fixed documented examples that `gwsr` rejects: the Model Armor helper examples (and the skills generated from them) and the README `--sanitize` example now use a valid project ID instead of `projects/P/...`; the README `calendar +freebusy` example uses `--end` (an all-day `--start` does not accept `--duration`); the README `drive +upload --convert` example uploads a `.docx` (PDFs have no Google format to convert to); and the README `modelarmor +create-template` mention includes its required flags. A new test runs every `gwsr` example in the README, CONTEXT.md and the skills with `--dry-run` so a rejected example fails CI.
