---
"gws-rust": patch
---

`gwsr auth login -s events` now requests the scopes the `events +subscribe` and `events +renew` helpers use (Chat messages, reactions, memberships and spaces, Meet spaces and Drive read scopes, plus Pub/Sub with `--write` or `--full`). It used to request only `drive.readonly`, because the Drive preset made `events` look covered. `-s workflow` (and `-s wf`) now requests the workflow helpers' scopes instead of failing to look up a Discovery document that does not exist. Neither service can make another one in the same `-s` list (such as `chat`) look covered any more.
