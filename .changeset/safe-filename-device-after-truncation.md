---
"gws-rust": patch
---

Downloaded file names (`drive +download`, `drive +export`, `drive +sync`, `gmail +download`) can no longer become a Windows device name such as `CON` or `nul.pdf` when a very long name is shortened: the device-name check now runs on the shortened name, so the file gets a `_` prefix instead of opening the device on Windows.
