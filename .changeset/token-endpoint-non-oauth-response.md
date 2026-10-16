---
"gws-rust": patch
---

A token-endpoint answer that is not an OAuth response is no longer treated as Google rejecting the credentials. Examples are a proxy's `407 Proxy Authentication Required`, a captive portal's redirect or HTML page (even with HTTP 200), and an empty error body. `gwsr auth status` used to report these as `"authenticated": false, "verified": true`. Commands failed with exit code 2 ("sign in again") and showed a JSON parse or Content-Type error instead of the HTTP status. They are now reported like a token-endpoint outage. `auth status` says `"verified": false` and puts the error in `verification_error`. Commands exit 1, or 6 for retryable statuses as before, with a message that names the HTTP status and says the credentials were not rejected. Real OAuth errors such as `invalid_grant` and `invalid_client` are unchanged.
