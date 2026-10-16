---
"gws-rust": patch
---

A token-endpoint answer that is not an OAuth error (reason `tokenEndpointError`) no longer comes with a hint that contradicts its message. A 401 like that, for example from a proxy, said "the credentials were not rejected" while the hint said "The access token was rejected. Sign in again". The hint now says that signing in again will not help, and points to the network path (a proxy or captive portal).
