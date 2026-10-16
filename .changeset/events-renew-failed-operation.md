---
"gws-rust": patch
---

`events +renew` no longer reports a subscription as renewed when the API returns a long-running operation that already finished with an error. The renewal (or `--reactivate`) now fails with the class of the operation's status code (for example exit 1 for PERMISSION_DENIED, exit 6 for UNAVAILABLE), alone or as one of the `--all` failures.
