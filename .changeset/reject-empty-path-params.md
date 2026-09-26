---
"gws-rust": patch
---

An empty path parameter, such as `--params '{"fileId":""}'`, is now a validation error (exit 3). Before this change it shortened the URL to the parent collection: `drive files delete` sent `DELETE .../drive/v3/files/`, and `gmail users messages get` requested `.../users/me/messages/`.
