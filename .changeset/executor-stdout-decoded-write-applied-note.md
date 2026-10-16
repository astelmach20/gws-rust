---
"gws-rust": patch
---

A generated method that changes something and whose decoded base64 payload cannot be written to stdout (`-o -`, for example on a full disk) now says in the error that the change was made and names the resource, as the JSON and `-o FILE` paths already did. Before, the error read as if nothing had been sent, so a retry repeated the write.
