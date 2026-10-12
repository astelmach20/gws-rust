---
"gws-rust": patch
---

`GWSR_REQUIRE_CONFIRM=1` now also gates the generated methods that send mail or messages, share files or run scripts (`gmail users messages send`, `gmail users drafts send`, `chat spaces messages create`, `drive permissions create`/`update`, `script scripts run`, `admin members insert`), share a calendar (`calendar acl insert`/`update`/`patch`), give others access to or route mail out of a mailbox (`gmail users settings delegates create`, `updateAutoForwarding`, `forwardingAddresses create`, `sendAs create`, `filters create`), or email event attendees (`calendar events insert`/`update`/`patch`/`move`/`quickAdd` with `sendUpdates` `all`/`externalOnly` or `sendNotifications=true`; without those parameters Calendar sends no notifications, so the call is not gated), both directly and inside `gwsr batch`. Previously only the `+` helpers were gated, so the policy could be bypassed by calling the raw method. These methods now accept `-y/--yes`.
