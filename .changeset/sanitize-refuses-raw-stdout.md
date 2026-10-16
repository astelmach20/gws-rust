---
"gws-rust": patch
---

`--sanitize` no longer lets unscreened content reach stdout. `-o -` on a generated method (an `alt=media` download, an export or any other raw payload) and `--output -` on `drive +download` and `drive +export` used to stream the raw bytes to stdout without Model Armor screening and exit 0, even in `block` mode. With `--sanitize` set, in either mode, these combinations are now refused with a validation error (exit 3) before any request is sent. Writing to a file (`-o PATH` / `--output PATH`) is unchanged.
