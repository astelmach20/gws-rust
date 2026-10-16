---
"gws-rust": patch
---

`events +subscribe` and `gmail +watch` no longer die on a Ctrl+C or SIGTERM that arrives while they are still creating their Pub/Sub topic and subscription. Before, the signal killed the process before the stream had installed its handlers, so `--cleanup` never ran and the resources were left behind. Now setup finishes, the stream stops before its first pull, and `--cleanup` deletes what was created (without `--cleanup`, the reconnect hint is printed as usual).
