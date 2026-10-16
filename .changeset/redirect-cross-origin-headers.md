---
"gws-rust": patch
---

A redirect from a Google API to another origin no longer carries the quota project or the request body. The `Authorization` header was already dropped, but `x-goog-user-project` and every other request header still went to the new host, and a 307/308 also re-sent the method and body there. Such a redirect is now followed as a plain GET (or HEAD) that keeps only `Accept`, `Accept-Language`, `Range` and `User-Agent`. A redirect that would keep another method, such as a 307/308 after a POST or PUT, is refused with a network error (exit 10) instead of sending the body to a host it was not meant for. Redirects within one origin behave as before.
