---
"gws-rust": patch
---

`drive permissions create`/`update` (and Drive v2's `permissions insert`/`update`/`patch`), directly or in `gwsr batch`, are now gated as destructive when they transfer ownership (`transferOwnership`, role `owner` or `pendingOwner`) or give a whole domain or anyone with the link a role above reader/commenter, as `drive +share` already was. They were only outbound, so without `GWSR_REQUIRE_CONFIRM=1` an ownership transfer went through without `--yes`. Drive v2's `permissions insert` and `patch` are now outbound like their v3 counterparts.
