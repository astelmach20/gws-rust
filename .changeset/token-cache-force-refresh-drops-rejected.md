---
"gws-rust": patch
---

When an API rejects a cached access token (HTTP 401) and the refreshed token comes back without an expiry, the rejected token is now removed from the token cache. Before, it stayed cached, and later commands were sent with it again until it expired.
