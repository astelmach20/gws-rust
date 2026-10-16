---
"gws-rust": patch
---

`gwsr auth login --profile NAME` no longer makes `NAME` the active profile when the default profile is already signed in. Since default-profile logins stopped writing `profile = "default"` to `config.toml`, the first login to any named profile took over as active. Every later command without `--profile` then quietly ran as that other account. A named login now becomes active only when nothing is configured and the default profile has no credentials. Use `gwsr auth use NAME` to switch.
