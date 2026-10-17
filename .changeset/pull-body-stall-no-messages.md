---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` now treat a Pub/Sub pull whose response body stops arriving for the pull timeout as "no messages yet", the same as a pull whose headers never arrive. Before, it was a network error that counted against `--max-failures`.
