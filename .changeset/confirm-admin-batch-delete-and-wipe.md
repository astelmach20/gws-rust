---
"gws-rust": patch
---

The confirmation gate now treats four irreversible `POST` methods as destructive, so they need `--yes` (or a prompt on a terminal) like any `DELETE`: Admin SDK `customers.chrome.printers.batchDeletePrinters` and `customers.chrome.printServers.batchDeletePrintServers`, and Cloud Identity `devices.wipe` and `devices.deviceUsers.wipe`. Before, they ran without confirmation, unlike `gmail.users.messages.batchDelete` and `people.people.batchDeleteContacts`.
