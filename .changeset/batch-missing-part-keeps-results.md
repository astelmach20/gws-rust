---
"gws-rust": patch
---

`gwsr batch` no longer throws away a whole group of results when the batch response lacks the part for one call. The calls it does answer were applied, so their result lines are printed; the error then names only the calls without a part (they may or may not have been applied) and the calls not sent. Before, every call in the group was reported as having no result, so re-running them could apply calls a second time.
