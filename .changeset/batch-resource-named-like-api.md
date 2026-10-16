---
"gws-rust": patch
---

`gwsr batch` finds a method named by its resource path when the API has a resource of its own name: `{"method": "tasks.list"}` in the `tasks` API (likewise `forms.get`, `people.get`) no longer fails as an unknown method.
