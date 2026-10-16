---
"gws-rust": patch
---

`admin +user-create --org-unit` now refuses a value that does not start with `/` (such as `Sales`) with a validation error (exit 3) before reading a password or sending anything. The Directory API takes an organizational unit's full path from the root (`/Sales`, `/Sales/EMEA`); before, a bare unit name was sent as is and the API rejected it.
