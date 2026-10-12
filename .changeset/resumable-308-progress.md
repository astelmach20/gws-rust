---
"gws-rust": patch
---

Resumable uploads (`--upload` above 5 MiB or with `--upload-resumable`, and `drive +upload`) no longer loop forever when the server keeps answering `308` without persisting anything: such responses now count as failed attempts and the upload gives up after the usual retries. A `Range` reaching past the end of the file is now a clear error instead of a crash, and a `308` that acknowledges every byte is finished with a valid `bytes */<size>` request instead of a malformed `Content-Range`.
