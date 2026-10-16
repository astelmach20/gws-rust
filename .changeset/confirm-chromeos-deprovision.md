---
"gws-rust": patch
---

The confirmation gate now treats deprovisioning a ChromeOS device as destructive, so it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`: `directory.chromeosdevices.action` with `action` `deprovision`, and `admin.customer.devices.chromeos.batchChangeStatus` with `changeChromeOsDeviceStatusAction` `CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_DEPROVISION`. A deprovisioned device loses its policies, device-level printers and kiosk use, and must be wiped before it can be enrolled again. Before, these ran without confirmation. Disabling and re-enabling a device are still not gated.
