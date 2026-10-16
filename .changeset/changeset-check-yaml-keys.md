---
"gws-rust": patch
---

The PR policy check now rejects changesets that `changeset version` cannot parse because a package is listed twice or a key's colon has no space after it (`gws-rust:patch`). These used to pass CI and then break the release job on main.
