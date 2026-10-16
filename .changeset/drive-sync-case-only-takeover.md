---
"gws-rust": patch
---

`drive +sync` on a case-insensitive file system (macOS, Windows) no longer reports a false "untracked" conflict when a new Drive item takes over a synced file's name with only a difference in case, such as `Report.txt` replaced by `report.txt`. If the local file is untouched, it is replaced, the same as a replacement under the exact same name. If it was edited, it is kept and reported as `modifiedLocally`. On a case-sensitive file system the two names are still separate files.
