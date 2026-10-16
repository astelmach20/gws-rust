---
"gws-rust": patch
---

A mistyped first word now gets the single closest suggestion across built-in commands and services. `gwsr auht` suggests `gwsr auth` instead of `vault`, and `gwsr hlep` suggests `gwsr help` instead of `keep`; `gwsr drvie` and `gwsr gmial` still suggest `drive` and `gmail`. Commands and services are ranked by the same edit distance, where a transposition counts as one edit, and a tie goes to the service.
