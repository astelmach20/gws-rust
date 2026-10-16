---
"gws-rust": patch
---
A required repeated parameter given as an empty array (for example `"permissions": []`) is now rejected instead of sending the request without it.
