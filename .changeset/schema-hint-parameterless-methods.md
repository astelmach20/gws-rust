---
"gws-rust": patch
---

The `--help` of a generated method that has no parameters now ends with its `Full request/response schema: gwsr schema <path>` line, as every other method's help does. These are mostly methods that take only a request body, such as `classroom courses create`, `sheets spreadsheets create` and `driveactivity activity query`, where the schema is the only description of what `--json` accepts.
