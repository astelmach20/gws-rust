---
"gws-rust": patch
---

`--page-items` with `--sanitize` in `warn` mode no longer drops the `_sanitization` annotation. Model Armor screens each page, and the page's annotation (a match, or a sanitization failure) is now attached to every item printed from that page. Before, the annotation was discarded with the page envelope, so the printed items looked screened and clean.
