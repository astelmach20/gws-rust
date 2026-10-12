---
"gws-rust": patch
---

An access token whose `expires_in` is absurdly large (up to 2^63-1 seconds) or negative no longer overflows the expiry computation. Debug builds used to panic, and a release build treated a service-account token with such a lifetime as already expired, so it was never reused from the token cache. The lifetime is now clamped, for both user refresh tokens and service accounts.
