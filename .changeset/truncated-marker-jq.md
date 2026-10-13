---
"gws-rust": patch
---

With `--page-limit` and `--jq`, the `{"_truncated":...}` marker that ends a truncated JSON run is no longer passed through the jq filter (where `--jq .name` printed `null`); it is printed as is.
