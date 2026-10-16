---
"gws-rust": patch
---

A rejected URL (`GWSR_API_BASE_URL`, a Discovery `rootUrl`, a request URL) whose unencoded password contains `?` or `#`, such as `https://user:pa#ss@proxy.example/`, no longer repeats the password in the error message. Those characters ended the part that was hidden, so the password was shown in full.
