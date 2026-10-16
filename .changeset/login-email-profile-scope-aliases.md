---
"gws-rust": patch
---

`gwsr auth login --scopes email,profile` now requests these OpenID aliases under the names Google reports them as granted (`userinfo.email`, `userinfo.profile`). Login no longer warns that `email` and `profile` "were not granted" when they were, and `requested_scopes` no longer lists each identity scope twice.
