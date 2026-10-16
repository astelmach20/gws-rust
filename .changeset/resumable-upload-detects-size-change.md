---
"gws-rust": patch
---
Resumable uploads now fail instead of uploading a truncated copy when the file grows (or shrinks) while it is being uploaded.
