---
"gws-rust": patch
---

When a `+helper` fails for a missing OAuth scope (403 `insufficientPermissions` / `ACCESS_TOKEN_SCOPE_INSUFFICIENT`) or an auth error, the hint now names the scopes the helper requested, for the active profile: for example `gwsr auth login --scopes calendar` for `calendar +insert`, or `gwsr auth login --scopes drive.readonly,chat.messages.create` for `workflow +file-announce`. Scopes the recorded grant already holds are left out. It used to suggest `gwsr auth login -s <service>`, which without a terminal grants only read-only scopes (so write helpers kept failing) and grants nothing for `workflow`.
