---
"gws-rust": patch
---

`script +push` drops a leading UTF-8 byte order mark (written by Notepad and PowerShell) from each local file instead of sending it as part of the source, where it became an invisible character at the start of the code and the first byte of the `appsscript.json` manifest. `+pull` never writes one, so a pushed file now round-trips unchanged.
