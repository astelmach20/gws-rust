---
"gws-rust": patch
---

`drive +upload --name` now refuses an empty or blank value with a validation error (exit 3) before credentials are loaded. Before, `--name ""` was sent as `name: ""` and Drive created the file as "Untitled".
