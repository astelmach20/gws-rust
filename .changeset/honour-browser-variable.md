---
"gws-rust": patch
---

`gwsr auth login` honours `$BROWSER` again on Linux and other Unix systems (it stopped when the browser launcher was rewritten to withhold `GWSR_*` variables). `$BROWSER` is a `:`-separated list of commands; each is split into words without a shell, `%s` is replaced by the URL (or the URL is appended), and the first command that starts wins before falling back to `open`, `xdg-open` or `wslview`. The browser still never sees any `GWSR_*` variable. A launcher that cannot be started for a reason other than not being installed no longer stops the search for the next one.
