---
"gws-rust": patch
---

`calendar +freebusy --slot --working-hours` no longer drops a whole day when the working-hours start or end falls in a daylight-saving gap (for example 02:00 on a spring-forward day). The time now shifts to the first valid local time after the gap.
