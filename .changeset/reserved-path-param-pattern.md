---
"gws-rust": patch
---

Generated commands now check `{+name}`-style path parameters against the Discovery `pattern` before sending anything. These parameters keep `/` in the URL, so a value like `groups/g1/memberships/m1` passed to `gwsr cloudidentity groups delete` used to send `DELETE v1/groups/g1/memberships/m1` and delete a membership instead of a group. Such values are now rejected with a validation error (exit 3) that names the expected pattern. `gwsr schema` also shows each parameter's `pattern`.
