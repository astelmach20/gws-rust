---
"gws-rust": patch
---

Method `--help` ("Full request/response schema: gwsr schema …") and the `Invalid --params` error now name a `gwsr schema` path that works. They used to print the Discovery method id, which fails for `events` (`workspaceevents.…`), `groupssettings` (`groupsSettings.…`), `alertcenter` / `cloudsearch` methods whose id skips a resource (`alertcenter.getSettings`), and every `<api>:<version>` service (`youtube.videos.list` instead of `youtube:v3.videos.list`).
