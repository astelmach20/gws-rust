---
"gws-rust": patch
---

`--format csv` and `--format table` now render the items of People `connections.list`, Gmail `history.list` and Drive `changes.list` responses as rows. Their list metadata (`totalPeople`, `historyId`, `newStartPageToken`) made the formatter treat the whole response as a single object, so the items were printed as one cell of JSON blobs.
