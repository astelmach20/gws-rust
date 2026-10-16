---
"gws-rust": patch
---

The confirmation gate now looks at the request body of two Admin SDK Directory device commands and treats a wipe as destructive, so it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`: `directory.mobiledevices.action` with `action` `admin_remote_wipe` or `admin_account_wipe`, and `admin.customer.devices.chromeos.issueCommand` with `commandType` `WIPE_USERS` or `REMOTE_POWERWASH`. Before, these ran without confirmation. Other actions of the same methods (approving or blocking a phone, rebooting a Chromebook) are still not gated.
