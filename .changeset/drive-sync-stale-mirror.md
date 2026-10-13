---
"gws-rust": patch
---

`drive +sync` no longer trusts local modification times. It used to treat a local file as current whenever its mtime was newer than Drive's, so a Drive file replaced by one with an older `modifiedTime` (a different file of the same name moved into the folder, or a revision uploaded with its original timestamp) left a stale copy reported as `unchanged`, and a local edit was silently overwritten once Drive changed.

`+sync` now keeps a sync-state file, `<dir>/.gwsr-sync.json`, keyed by Drive file ID: the Drive `modifiedTime`, `version`, size and checksums last mirrored, and the local size, mtime and SHA-256 written. A file is downloaded again when its Drive copy changed or another Drive item took its name, and only if the local copy is unchanged since the last sync. A local file that was edited, or that the sync did not write, is kept and listed under the new `conflicts` key (reason `modifiedLocally` or `untracked`); the command then exits `1` with reason `syncConflict` after printing the report. The new `--overwrite` flag replaces such files with the Drive copy.

Behavior change: on the first run after upgrading there is no state file, so existing local files are reported as `untracked` conflicts unless Drive's SHA-256 checksum shows they are identical (Google Docs exports have none). Run once with `--overwrite` to take the Drive copies, or move the files aside. A corrupt state file, or one for another folder, is reported on stderr and treated the same way.
