---
"gws-rust": patch
---

`admin users update`/`patch` now require confirmation (`--yes`, or a prompt on a terminal) when the body sets `suspended` to `true`, like `admin +user-suspend`; other user edits and unsuspending are not gated. More Classroom methods are now gated as outbound (confirmation required when `GWSR_REQUIRE_CONFIRM=1`), directly or in `gwsr batch`: `classroom invitations create` (it emails the invitee), `classroom courses students create` and `classroom courses teachers create` (they add people to a course), and `classroom courses announcements create` (it posts to the whole class). These commands now take `--yes`.
