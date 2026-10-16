---
"gws-rust": patch
---

When `--sanitize` withholds the output of `workflow +email-to-task`, `workflow +file-announce`, `gmail +label`, `+archive`, `+trash` or `+filter delete`, the error now says that the request succeeded and the change was made, and names the created task or Chat message. Before, it read like a refusal, so a retry created a duplicate task or posted the announcement twice.
