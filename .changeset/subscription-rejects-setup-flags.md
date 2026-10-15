---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` now reject setup flags that have no effect with `--subscription`, instead of silently ignoring them. With an existing subscription nothing is created, so `--cleanup` deleted nothing, and `--project`, `--topic` and `--label-ids` (`+watch`) or `--target`, `--event-types` and `--project` (`+subscribe`) were dropped. These combinations now fail with a validation error (exit `3`). The `events +subscribe` help example that combined `--subscription` with `--cleanup` now creates its resources with `--target`, `--event-types` and `--project`.
