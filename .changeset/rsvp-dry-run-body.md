---
"gws-rust": patch
---

`calendar +rsvp --dry-run` now shows the PATCH body a real run sends: the event's whole guest list with only your entry changed, and a `comment` only when `--comment` is given. The plan used to list your entry alone with `"comment": null`, which describes a request that would remove every other guest and clear your existing comment.
