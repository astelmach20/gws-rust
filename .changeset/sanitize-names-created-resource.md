---
"gws-rust": patch
---
When `--sanitize` withholds the output of a command that already changed something (for example `admin +user-create`), the error now says the change was made and names the resource's IDs, without showing the withheld content.
