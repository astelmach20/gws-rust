---
"gws-rust": patch
---

`admin-reports +audit --application` now accepts every application the Reports API lists in its Discovery document, such as `contacts`, `takeout`, `voice`, `meet_hardware` and `admin_data_action`. The helper used a hard-coded list of 25 applications and refused the rest, even though the API supports them.
