---
"gws-rust": patch
---

A bare `--page-items` now picks `usageReports` for Admin Reports usage methods. It used to fail with "several array fields (usageReports, warnings)" because it treated the report's `warnings` array as a second list. `--page-items=warnings` still selects the warnings explicitly.
