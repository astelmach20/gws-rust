---
"gws-rust": patch
---

`gmail +watch --subscription` accepts `--label-ids` again. The labels filter which messages are emitted, so they matter with an existing subscription too, but since the change that rejected setup flags with `--subscription`, the command refused them and could only stream every added message (sent mail, drafts, other labels). The `reconnect` hint printed after setup now includes the `--label-ids` the run used.
