---
"gws-rust": patch
---

`--json` bodies may set a field or map entry to `null` to clear it (for example, removing a Drive `appProperties` key on `files update`). Before this fix, schema validation rejected these bodies with "Expected type ..., found null".
