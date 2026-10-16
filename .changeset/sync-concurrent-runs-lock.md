---
"gws-rust": patch
---
`drive +sync` now locks its directory (`.gwsr-sync.lock`) and refuses a second run into the same directory with exit 3, instead of letting the run that saves last drop the sync state the other recorded.
