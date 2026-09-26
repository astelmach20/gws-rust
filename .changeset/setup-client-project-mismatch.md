---
"gws-rust": patch
---

`gwsr auth setup --non-interactive` no longer reports success when the saved OAuth client belongs to a different GCP project than the one being set up. It now returns `action_required` with steps to replace the client, since the old client's project would otherwise stay the quota project.
