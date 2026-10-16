---
"gws-rust": patch
---

Gmail `+send`, `+reply`, `+reply-all`, `+forward` and `--draft` now give the message upload the same size-scaled time limit as other media uploads (the response timeout plus one second per 64 KiB). Before, a large message with attachments on a slow link could hit the plain `--timeout` (60 seconds by default) while it was still being sent. That failed the command even though Gmail might have sent the message.
