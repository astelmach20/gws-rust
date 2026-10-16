---
"gws-rust": patch
---

Text read with `docs +create/+write --text-file`, `sheets +append/+write --csv-file` and `admin +user-create --password-file` (from a file or `-` for stdin) no longer keeps a leading UTF-8 byte order mark, which Windows editors and PowerShell write. Before, `docs` inserted it into the document as an invisible character, and with `--markdown` it stopped a first-line `# Heading` from becoming a heading.
