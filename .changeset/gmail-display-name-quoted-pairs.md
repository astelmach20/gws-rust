---
"gws-rust": patch
---

Gmail helpers no longer corrupt display names that contain escaped quotes or backslashes. Replying to `"Bob \"The Builder\"" <bob@example.com>` used to send the reply to `"Bob \\\"The Builder\\\""`, so every round trip added more backslashes. The recipient now sees `Bob "The Builder"`. A display name like `"A" and "B"` is also no longer turned into `A" and "B`.
