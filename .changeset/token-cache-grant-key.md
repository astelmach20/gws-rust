---
"gws-rust": patch
---

Access tokens are now cached per grant, not only per OAuth client. After `gwsr auth login`, a command that was already running with the previous login's refresh token could cache that grant's access token, and later commands would use it (possibly for another account or scope set). The cache key now includes a short SHA-256 fingerprint of the refresh token (never the token itself), so such an entry is never served to the new login. Tokens cached before this change are ignored and expire on their own.

The cross-process lock also now notices when its lock file was deleted while waiting (as `gwsr auth logout` does): it relocks the current file, or fails with an error if the profile directory is gone, instead of holding a lock that excludes nobody.
