---
"gws-rust": patch
---

Paginated `--format csv` and `--format table` output (`--page-all`, `--page-items`) now aligns every row to the header printed with the first page. Previously each page computed its own columns, so a later page whose items had a missing, extra or reordered field printed values under the wrong headers. The first page with rows fixes the columns (the `--columns` selection, or that page's fields); later rows get empty cells for missing fields, and a field that first appears on a later page is left out with a warning on stderr naming it (use `--columns` or `--format json` to keep it). Table widths also stay fixed across pages, and `--page-items` items render as table rows instead of one key/value block per item.
