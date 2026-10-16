---
"gws-rust": patch
---

Tests only: the `auth logout` test now checks that an already-revoked token is reported as revoked, and that `--no-revoke` sends no revocation request.
