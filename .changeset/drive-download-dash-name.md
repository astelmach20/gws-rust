---
"gws-rust": patch
---
`drive +download` without `--output` saves a Drive file named `-` to a local file instead of streaming its bytes to stdout followed by a JSON summary.
