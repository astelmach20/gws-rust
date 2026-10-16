---
"gws-rust": patch
---

Tests only: four helper rejection tests now check the specific error and that no request was sent. Before, a request the mock server didn't match also failed (404), so they passed even without the validation they were named after.
