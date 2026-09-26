---
"gws-rust": patch
---

`events +subscribe` now deletes the Pub/Sub topic and subscription it just created if a later setup step fails, for example when the Workspace Events API rejects the subscription. Before, the command exited with an error and left both resources behind, even with `--cleanup`. Nothing publishes to them, and no reconnect hint named them. If the Workspace Events operation is still running, the resources are kept.
