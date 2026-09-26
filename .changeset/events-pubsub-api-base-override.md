---
"gws-rust": patch
---

`events +subscribe`, `events +renew` and `gmail +watch` now send their Workspace Events and Pub/Sub requests to the `GWSR_API_BASE_URL` override when it is set. They used to send them to Google's hosts.
