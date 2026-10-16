---
"gws-rust": patch
---

Auth and missing-scope hints for Chat methods no longer tell you to sign in with an app-only scope such as `chat.bot` or `chat.import`, which Google refuses in a user sign-in. They now name the first scope a user can grant, such as `chat.messages`.
