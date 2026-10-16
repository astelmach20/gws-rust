---
"gws-rust": patch
---

A token-endpoint answer that is not an OAuth error, such as a proxy's `407 Proxy Authentication Required`, a captive portal's redirect or HTML page, or an empty error body, is no longer treated as Google rejecting the credentials. `gwsr auth status` used to report it as `"authenticated": false, "verified": true`, and commands failed with exit code 2 ("sign in again") and a JSON parse error instead of the HTTP status. It is now reported like a token-endpoint outage: `auth status` says `"verified": false` with the error in `verification_error`, and commands exit 1 with a message naming the HTTP status and stating that the credentials were not rejected (exit 6 for retryable statuses, as before). Real OAuth errors such as `invalid_grant` and `invalid_client` are unchanged.
