---
"gws-rust": patch
---

`drive +upload --mime-type` is now validated before credentials are loaded: a value with control characters (such as an embedded newline) or that is not a `type/subtype` media type is a validation error (exit 3). It used to be sent as the `X-Upload-Content-Type` header and fail only when the request was built, as an internal error (exit 5).
