---
"gws-rust": patch
---

Generated commands now ask for JSON from APIs whose Discovery document defaults to another response format. The Groups Settings API (`gwsr groupssettings groups get|update|patch`) defaults `alt` to `atom`, so its responses came back as Atom XML instead of JSON. `gwsr` now sends `alt=json` when the document's default `alt` is not `json` and the caller did not set `alt` in `--params`. `--dry-run` shows the added parameter.
