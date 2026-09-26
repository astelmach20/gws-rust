---
"gws-rust": patch
---

Profile names must now be lowercase (`[a-z0-9_.-]`). On case-insensitive file systems (macOS and Windows by default) `Work` and `work` were the same profile directory, so `gwsr auth login --profile work` silently replaced the credentials of profile `Work`, `gwsr auth use work` switched to them, and `gwsr auth list` showed no active profile. **Breaking:** a profile with uppercase letters in its name is now rejected; log in again under a lowercase name.
