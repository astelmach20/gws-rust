---
"gws-rust": patch
---

`gwsr schema` now shows the full structure of request and response bodies: the fields of inline nested objects (for example Drive `File.contentHints.thumbnail`), map values (`additionalProperties`, as in `File.appProperties`), items of nested arrays (Sheets `ValueRange.values`), and property enums and defaults. Previously these were printed as a bare `"type": "object"` or `"type": "array"`, although `--json` bodies are validated against them. `--resolve-refs` also follows references inside those nested objects.
