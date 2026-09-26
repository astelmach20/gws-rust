---
"gws-rust": patch
---

`script +pull` keeps dots in Apps Script file names: `config.dev` is now saved as `config.dev.gs` instead of `config.gs`. Before, two files such as `config.dev` and `config.prod` were written to the same local file (the second overwrote the first with `--overwrite`, or failed after a partial write without it), and `+push` renamed them on the server.
