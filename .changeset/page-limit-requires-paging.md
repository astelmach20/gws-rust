---
"gws-rust": patch
---

`--page-limit` and `--page-delay` on a generated list method now fail with a validation error (exit 3) unless `--page-all` or `--page-items` is also given. Before, they were silently ignored and only the first page was fetched, so `--page-limit 5` alone returned one page. `page_limit` / `page_delay_ms` set in `config.toml` or the environment still apply only to paged runs and are not rejected.
