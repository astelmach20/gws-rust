---
"gws-rust": patch
---

`events +subscribe` now fills each event's `type`, `source` and `time` from the `ce-type`, `ce-source` and `ce-time` Pub/Sub attributes that Workspace Events actually sends (the CloudEvents Pub/Sub binding). Before, it read unprefixed `type`/`source`/`time` attributes, so these fields were always `null` and `--jq 'select(.type == ...)'` matched nothing.
