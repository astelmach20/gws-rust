---
"gws-rust": patch
---

The JSON error envelope on stderr now escapes C1 control characters (U+0080–U+009F), DEL and bidi, zero-width and line-separator characters as `\uXXXX`. JSON already escaped ESC, but these characters were written raw, and some terminals act on them: U+009B is an 8-bit CSI. Error messages carry API- and filter-derived text such as a `--jq` `error(...)` value. The decoded JSON is unchanged. In the human error form (`--format table|yaml|csv`), argument-parsing errors are now sanitized like every other error, because clap quotes argument values verbatim. As a result, those errors are no longer colored.
