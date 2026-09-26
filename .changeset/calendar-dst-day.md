---
"gws-rust": patch
---

`gwsr calendar +agenda --today` / `--tomorrow` and `gwsr workflow +standup-report` now cover the whole local calendar day, from local midnight to the next local midnight. On daylight-saving transition days the window used to be a fixed 24 hours, so it dropped the last hour of a 25-hour fall-back day and spilled an hour into the next day after a 23-hour spring-forward day. Days whose midnight does not exist (zones that spring forward at midnight) now start at the first valid local time instead of failing.
