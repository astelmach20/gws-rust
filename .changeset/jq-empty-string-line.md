---
"gws-rust": patch
---

A `--jq` filter whose only result is an empty string now prints an empty line, as `jq -r` does, instead of nothing. With `--page-items` or `--page-all`, an item or page whose result was `""` (a file with an empty `description`, say) was dropped from the stream, so the output had fewer lines than items.
