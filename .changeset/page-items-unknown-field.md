---
"gws-rust": patch
---

`--page-items=FIELD` now fails with a validation error (exit 3) that lists the response's list fields when `FIELD` appears neither in the response schema nor in the page. Before this fix, a typo such as `--page-items=file` for `drive files list` printed nothing and exited 0.
