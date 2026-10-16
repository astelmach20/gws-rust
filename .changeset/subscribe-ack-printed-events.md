---
"gws-rust": patch
---

`events +subscribe` now acknowledges the events it already printed or wrote when a later event in the same pull fails (a `--jq` error, a closed stdout pipe such as `| head -n1`, a failed `--output-dir` write or a Model Armor failure). Before, nothing in the batch was acknowledged, so Pub/Sub redelivered the events that had been written and the stream printed them again.
