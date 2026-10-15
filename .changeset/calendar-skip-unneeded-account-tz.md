---
"gws-rust": patch
---
`calendar +insert` and `+update` no longer fetch the account time zone when every `--start`/`--end` has a UTC offset or is a date (or only other fields change), so a failing time zone lookup can no longer fail those commands.
