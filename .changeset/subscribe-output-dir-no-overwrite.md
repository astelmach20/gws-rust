---
"gws-rust": patch
---

`events +subscribe --output-dir` no longer silently overwrites an existing event file. Files are named `<milliseconds>_<counter>.json`, and the counter restarts with every run, so two runs writing to the same directory could pick the same name (same millisecond, same counter) and the second one replaced the first run's event. Now a name that is already taken is skipped in favour of the next free counter value, and files are created without overwriting.
