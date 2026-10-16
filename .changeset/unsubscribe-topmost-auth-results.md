---
"gws-rust": patch
---

`gmail +unsubscribe` now trusts only Gmail's own `Authentication-Results` header (the topmost one whose authserv-id is exactly `mx.google.com`) when checking for `dkim=pass`. A sender-supplied header lower in the message that claims `mx.google.com; dkim=pass` no longer enables the one-click POST, and `dkim=pass` must be an actual DKIM result rather than text inside a comment, another method's value or a look-alike authserv-id such as `mx.google.com.example.com`.
