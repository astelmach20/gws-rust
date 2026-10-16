---
"gws-rust": patch
---

`events +subscribe` acknowledges the events it already printed when a Ctrl+C or SIGTERM arrives partway through a pulled batch (for example while `--sanitize` screens the next event). Before, the signal cancelled the whole batch before its acknowledgement, so Pub/Sub redelivered those events and a reattached stream printed them twice. A signal now cancels only a pending pull; the rest of the batch is not printed and stays unacknowledged.
