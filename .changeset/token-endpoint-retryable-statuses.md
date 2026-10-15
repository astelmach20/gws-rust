---
"gws-rust": patch
---

Google's OAuth token endpoint failures are now classified with the same retryable-status set as API calls (408, 429, 500, 502, 503, 504). A token-endpoint 408 now exits `6` (`tokenEndpointUnavailable`, retryable) instead of `2` as a credential error, and a permanent 5xx such as 501 or 505 now exits `1` with reason `tokenEndpointError` and a message that no longer says "temporarily unavailable … retry later".
