---
"gws-rust": patch
---
Resumable upload chunk retries now honor `Retry-After`: they wait as long as the server asks, and stop at once (exit 6) when it asks for more than 60 seconds, instead of re-sending the chunk on gwsr's own short backoff.
