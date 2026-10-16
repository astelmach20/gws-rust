---
"gws-rust": patch
---

Helper commands (`+share`, `+upload`, `+sync`, `+agenda` and the rest) are now offered only for the API version they were written for. Before, `gwsr drive:v2 +share` or `--api-version v2` ran the Drive v3 helpers against the v2 endpoints, sending v3 field names (`emailAddress`, `sendNotificationEmail`, `name`) that v2 does not accept and reading a `files` list that v2 does not return. Another version now gets only its generated Discovery commands.
