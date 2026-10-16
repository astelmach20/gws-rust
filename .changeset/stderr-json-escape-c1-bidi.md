---
"gws-rust": patch
---

Stream diagnostics (`gmail +watch`, `events +subscribe`) and the default JSON log lines on stderr now escape C1 control characters (U+0080–U+009F), DEL and bidi, zero-width and line-separator characters as `\uXXXX`, as the error envelope already did. These lines carry API- and message-derived text, and some terminals act on those characters: U+009B is an 8-bit CSI. The decoded JSON is unchanged.
