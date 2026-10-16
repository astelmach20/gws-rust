---
"gws-rust": patch
---
The npm launcher now installs its signal-forwarding handlers before it starts `gwsr`, so a SIGINT/SIGTERM/SIGHUP/SIGQUIT that arrives just after the binary starts is forwarded to it instead of killing the launcher and orphaning the binary.
