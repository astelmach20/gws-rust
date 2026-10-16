---
"gws-rust": patch
---

`gwsr commands <SERVICE> --api-version V` now lists the commands of version `V` and reports it as `version`. The flag used to be accepted and ignored, so the inventory showed the default version. `gwsr commands --api-version V` with no service named is now a validation error (exit 3): one version cannot apply to every API.
