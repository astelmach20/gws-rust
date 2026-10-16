---
"gws-rust": patch
---
`gmail +unsubscribe` now exits 10 (network error) instead of 5 (internal error) when the sender's one-click unsubscribe server can't be reached (connection refused, DNS, TLS or timeout).
