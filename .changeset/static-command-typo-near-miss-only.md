---
"gws-rust": patch
---

An unregistered API named without a version (`gwsr dlp`, `gwsr oauth2`) is reported as an unknown service again, with the hint to use `<api>:<version>`, instead of "Did you mean `gwsr help`?" or "`gwsr auth`?". The suggestion for a mistyped built-in command (`gwsr completion`, `gwsr comands`) now only appears for a near miss: one edit, or one per four characters.
