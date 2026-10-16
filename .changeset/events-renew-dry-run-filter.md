---
"gws-rust": patch
---

`gwsr events +renew --subscription-id ID --event-types ... --dry-run` no longer shows a `filter` in its plan. A named subscription is renewed directly without listing subscriptions, so `--event-types` only selects the OAuth scope there. The filter is now shown only with `--all`.
