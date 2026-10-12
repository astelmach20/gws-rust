---
"gws-rust": patch
---

calendar `+insert --duration` and `+update` (keeping an event's length when only `--start` changes) now add elapsed time across a daylight-saving change. A two-hour event starting at 00:30 on a fall-back night used to end at 02:30 wall-clock time (three real hours), and a one-hour event starting at 01:30 before a spring-forward change was rejected because 02:30 does not exist; both now end exactly the requested duration later.
