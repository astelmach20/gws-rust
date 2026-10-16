---
"gws-rust": patch
---
`gwsr batch` now sends `supportsAllDrives=true` by default on methods that have it, like the generated commands, so batch calls on shared-drive files no longer fail with 404.
