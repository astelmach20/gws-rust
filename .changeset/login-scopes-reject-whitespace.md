---
"gws-rust": patch
---

`gwsr auth login --scopes` now rejects an entry that contains whitespace, control or non-ASCII characters (for example `--scopes "gmail.modify drive"`) with a validation error asking for comma-separated scopes. Before, such an entry was recorded as one bogus scope, while Google read it as several, so the "not granted" warning and the saved scope list were wrong.
