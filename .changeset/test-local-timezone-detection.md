---
"gws-rust": patch
---

Tests only: the local-timezone fallback test now checks that the zone the OS reports is the one used. Before, it skipped every assertion whenever lookup failed, including when the lookup itself was broken.
