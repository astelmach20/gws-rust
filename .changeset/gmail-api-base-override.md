---
"gws-rust": patch
---

Gmail helpers (`+send`, `+reply`, `+reply-all`, `+forward`, `+read`, `+triage`, `+search`, `+label`, `+archive`, `+trash`, `+filter`, `+unsubscribe`, `+attachments` and the Gmail calls of `+watch`) now follow `GWSR_API_BASE_URL`, both in real runs and in their `--dry-run` plans. Before, they always went to `https://gmail.googleapis.com`, ignoring the configured endpoint that generated Gmail methods and the other helpers use.
