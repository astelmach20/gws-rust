---
"gws-rust": patch
---

A 429 or 5xx from Google's OAuth token endpoint (refreshing a login, a service-account token, or the login code exchange) now exits `6` (retryable, reason `tokenEndpointUnavailable`) instead of `2` ("sign in again"). The credentials were not rejected, so agents should retry with backoff rather than ask the user to log in. The error keeps only a short excerpt of the endpoint's response body.
