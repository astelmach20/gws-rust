---
"gws-rust": patch
---

Gmail `+reply`, `+reply-all` and `+forward` no longer take an address from a `<...>` inside a comment or a quoted display name. `a@example.com (Support <team>)` was read as the address `team`, and `Alice <alice@example.com> (via <relay@example.net>)` as `relay@example.net`, so a reply could go to the wrong recipient.
