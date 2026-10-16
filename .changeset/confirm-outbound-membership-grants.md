---
"gws-rust": patch
---

More generated methods that notify people or grant them access are now outbound, so `GWSR_REQUIRE_CONFIRM=1` gates them (directly and in `gwsr batch`) and their commands take `--yes`. The new ones are `chat.spaces.members.create`, `chat.spaces.setup`, `meet.spaces.members.create`, `cloudidentity.groups.memberships.create` (the Cloud Identity counterpart of `directory.members.insert`), `directory.users.makeAdmin`, `directory.roleAssignments.insert` and `gmail.users.settings.sendAs.verify`, which emails the send-as address. Before, they were sent without confirmation.
