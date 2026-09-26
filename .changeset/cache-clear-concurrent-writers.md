---
"gws-rust": patch
---

`gwsr cache clear` no longer fails when another `gwsr` process uses the Discovery cache at the same time. A file that disappears before it is deleted (a concurrent write finished, or another clear removed it) is skipped instead of reported as an error, and documents cached while the clear runs are removed too instead of failing with "Directory not empty". Other failures, such as permission errors, are still reported. The reported count is the number of documents this clear actually removed.
