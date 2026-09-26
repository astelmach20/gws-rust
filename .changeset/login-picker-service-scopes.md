---
"gws-rust": patch
---

`gwsr auth login` scope picker: services passed with `-s` that are not in the list (the basic list, or an API the project has not enabled) now have their scopes looked up and listed, so `gwsr auth login -s people` offers the Contacts scopes instead of showing only `cloud-platform` and silently dropping People. The read-only and read/write templates include those scopes (admin-only ones excluded). The "Full access" template no longer adds `cloud-platform` behind the user's back: it is now shown as a row that the template ticks and the user can untick, and the picker requests exactly the ticked rows.
