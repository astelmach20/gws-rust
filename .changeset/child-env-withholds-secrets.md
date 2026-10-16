---
"gws-rust": patch
---

Programs gwsr starts no longer inherit its environment variables. `gcloud` (run by `gwsr auth setup`) and the browser opened by `gwsr auth login` used to receive the whole environment, `GWSR_TOKEN`, `GWSR_CLIENT_SECRET` and the build-time `GWSR_DEFAULT_CLIENT_SECRET` included; every `GWSR_*` variable is now withheld from them, and the rest of the environment is passed on. The browser is now opened with the platform launcher directly (`open` on macOS, `xdg-open` or `wslview` elsewhere on Unix, `rundll32 url.dll,FileProtocolHandler` on Windows) instead of through the `webbrowser` crate, which started some launchers itself with the full environment.
