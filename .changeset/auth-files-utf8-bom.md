---
"gws-rust": patch
---

`GWSR_TOKEN_FILE`, `GWSR_CREDENTIALS_FILE` and Application Default Credentials files that start with a UTF-8 byte order mark are now read. Windows PowerShell 5.1 (`Set-Content -Encoding UTF8`, `Out-File`) and older Notepad write one. Before, the token was refused as containing non-ASCII characters and the JSON was refused as invalid.
