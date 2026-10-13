---
"gws-rust": patch
---

`-o/--output` waits for a long-running operation on its own only for methods that return one, so saving a resource that merely looks like an operation (e.g. a People contact group, with `name` and `metadata`) no longer fails with "--wait: the people API has no operations.get method to poll".
