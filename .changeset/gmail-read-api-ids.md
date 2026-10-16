---
"gws-rust": minor
---

`gmail +read` JSON output now includes the Gmail API `id` and `threadId` (the same names `+search` and `+triage` use), so `.id` can be passed straight to `+reply`, `+reply-all`, `+forward` and other `--message-id` flags. Breaking: the RFC 5322 `Message-ID` header moves from `message_id` to `message_id_header`, so it can no longer be mistaken for the API ID, and `thread_id` is renamed to `threadId`.
