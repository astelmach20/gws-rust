---
"gws-rust": patch
---

A generated method's raw payload streamed to stdout (`-o -`, for example `drive files get --params '{"fileId":"…","alt":"media"}' -o -`) now arrives complete and in order. Before, large payloads could be written with their chunks out of order, and a failing stdout (a full disk, a file size limit) exited 0 with a truncated payload instead of reporting the error (exit 5). A reader that closes the pipe (`| head -c 100`) now also stops the download right away instead of letting it run to the end.
