---
"gws-rust": patch
---
On Windows, a `gwsr` process waiting for the token-cache or key lock no longer ends up "holding" a lock on a file that `auth logout` deleted meanwhile, which let two processes into the locked section at once; like on Unix, it now reopens and locks the file that is actually at the path.
