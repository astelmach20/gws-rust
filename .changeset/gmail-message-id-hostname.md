---
"gws-rust": patch
---

Messages built by `gmail +send`, `+reply`, `+reply-all` and `+forward` no longer put this computer's host name in the generated `Message-ID` (visible in `--dry-run` plans and saved drafts); the ID now ends in `@localhost`.
