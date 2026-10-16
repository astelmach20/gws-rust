---
"gws-rust": patch
---

`drive +upload --folder-id`, `drive +share --email`/`--domain`, `script +push/+pull/+run/+logs --script-id` and `tasks +add --title` now refuse an empty or blank value with a validation error (exit 3) before anything is sent. Before, the empty value went to the API as `parents: [""]`, `emailAddress: ""`, `domain: ""`, `scriptId=` or `title: ""`.
