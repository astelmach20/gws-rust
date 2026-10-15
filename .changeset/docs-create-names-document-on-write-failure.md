---
"gws-rust": patch
---

`docs +create --text`/`--text-file`: when the document is created but writing its content fails, the error now names the new document (id and URL) and says to add the content with `docs +write --document-id`, instead of a bare API error that made a rerun of `+create` leave an empty duplicate document behind. The error keeps its class and exit code.
