---
"gws-rust": patch
---

The shell completion callback now handles a failed write to stdout like every other command. A reader that closed the pipe is a quiet exit 0, where before gwsr printed a `Broken pipe` error envelope and exited 3. Any other write failure, such as a full disk, is reported as `failed to write to stdout` with exit 5, no longer as a validation error (exit 3).
