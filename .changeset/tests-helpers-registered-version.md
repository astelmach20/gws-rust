---
"gws-rust": patch
---

Tests only: three helper tests (`gmail +filter create` flag rules, `events +renew` flag rules, and the check that every helper tip in the generated skills is a whole sentence) built their command tree without an API version. Since helpers are offered only for the registered version, they found no helper and passed without checking anything. They now use the registered version and fail if the helper is missing. No behavior change.
