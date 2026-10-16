---
"gws-rust": patch
---
`gwsr batch` now accepts a Discovery method id whose prefix is not the API name, such as `directory.users.get` for `gwsr batch admin`, instead of reporting an unknown method.
