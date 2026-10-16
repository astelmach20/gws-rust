---
"gws-rust": patch
---
`--wait` on an API whose `operations.get` is nested (storage `b/{bucket}/operations/{operationId}`) now puts the operation id in the parameter after `operations/` and fills the other path parameters from the request, and refuses before sending when it cannot.
