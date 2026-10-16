---
"gws-rust": patch
---

More generated methods are now gated as outbound (confirmation required when `GWSR_REQUIRE_CONFIRM=1`), directly or in `gwsr batch`: `script projects deployments create`/`update` (they publish Apps Script code for others to run), `classroom userProfiles guardianInvitations create` (it emails the guardian), `drive approvals comment` (it notifies the initiator and reviewers), `gmail users settings updateVacation` when the body turns the auto-reply on, and `groupssettings groups update`/`patch` when the body lets anyone join or view the group (`whoCanJoin` `ANYONE_CAN_JOIN`, `whoCanViewGroup` `ANYONE_CAN_VIEW`) or allows external members (`allowExternalMembers` `true`). These commands now take `--yes`.
