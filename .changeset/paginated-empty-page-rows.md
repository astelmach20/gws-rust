---
"gws-rust": patch
---

`--page-all --format csv|table` no longer turns a page without items (Google omits empty lists) into a row of `kind`/`nextPageToken` metadata, which on the first page fixed the header to those fields and dropped every real item that followed.
