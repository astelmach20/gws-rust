---
"gws-rust": patch
---
The debug log line for a token refresh after HTTP 401 no longer records the request's query string, such as a resumable upload's `upload_id`.
