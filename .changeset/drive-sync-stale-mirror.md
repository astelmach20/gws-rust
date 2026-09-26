---
"gws-rust": patch
---

`drive +sync` no longer keeps a stale local copy when a Drive file is replaced by one with an older modification time, for example a different file of the same name moved into the folder or uploaded with its original timestamp. Downloads now carry the Drive `modifiedTime`, and a local file is replaced whenever its modification time or size differs from the Drive copy.

Behavior change: a mirrored file edited locally is now overwritten on the next run. Previously it was kept whenever its local modification time was newer. Files mirrored by an earlier version are downloaded once more on the first run, because their timestamps are the download time.
