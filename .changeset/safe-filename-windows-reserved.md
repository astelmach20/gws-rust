---
"gws-rust": patch
---

Files named after remote data (`gmail +attachments`, `drive +download`, `drive +sync`) no longer use Windows device names such as `CON.pdf`, `nul` or `COM1.txt`: they get a `_` prefix (`_CON.pdf`) on every platform, so the name is the same everywhere and Windows writes a real file instead of the device. Trailing dots and spaces, which Windows drops, are stripped. `script +pull` refuses an Apps Script file whose name is a Windows device name, since renaming it on disk would break `+push`.
