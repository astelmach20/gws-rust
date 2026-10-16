---
"gws-rust": patch
---

Shell completions now complete local paths for flags that take a file or directory (`--upload`, `-o/--output`, `--file`, `--attach`, `--dir`, `--output-dir`, `batch --input` and others). Before, these flags completed nothing, so zsh, fish, PowerShell and elvish offered no file names at all (bash only fell back to its own file completion).
