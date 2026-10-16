---
"gws-rust": patch
---

Tests only: three validation property tests now generate inputs that reach the accept path and compare against an independent definition. Before, the API base URL and `GWSR_API_BASE_URL` properties never saw an accepted URL, and the dangerous-character property restated the function under test.
