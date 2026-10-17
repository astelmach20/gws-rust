---
"gws-rust": patch
---

`gmail +triage` help no longer calls it an "inbox" summary. Its default query, `is:unread`, searches all mail, archived messages included. The help now says so and shows `--query 'is:unread in:inbox'` for the Inbox only. Behavior is unchanged.
