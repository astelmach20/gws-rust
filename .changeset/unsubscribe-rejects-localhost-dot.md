---
"gws-rust": patch
---

`gmail +unsubscribe` now refuses a one-click URL whose host is `localhost.` (with a trailing dot), any `*.localhost` name, or a single-label name written with a trailing dot (`intranet.`). The check for loopback and single-label hosts compared the host verbatim, so these spellings got past it. A DKIM-signed message could then make gwsr POST to a service on your own machine or network.
