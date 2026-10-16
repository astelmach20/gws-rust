---
"gws-rust": patch
---

`gmail +forward` keeps the `charset` of the original message's attachments. Gmail reports a part's type without parameters, so a forwarded `notes.txt` sent as `text/plain; charset=windows-1252` went out as plain `text/plain`, which mail clients read as US-ASCII and show garbled. A charset that is not a valid MIME token is left out, with a warning.
