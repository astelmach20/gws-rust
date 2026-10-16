---
"gws-rust": patch
---

A token-endpoint answer that is a JSON object but no usable token response (for example a `200 OK` without an `access_token`) is no longer reported as rejected credentials (exit 2, "sign in again"). Like an HTML page from a proxy, it is now a token-endpoint failure (`tokenEndpointError`, exit 1, or 6 for a retryable status) that keeps the HTTP status and says the credentials were not rejected. This applies to user refresh tokens, the login code exchange and service-account keys.
