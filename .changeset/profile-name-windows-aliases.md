---
"gws-rust": patch
---

Profile names can no longer end with `.` or be a Windows device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`, with or without an extension). Windows drops trailing dots from directory names, so `gwsr auth login --profile work.` wrote to the `work` profile's directory and replaced its credentials, and a device name opened the device instead of a profile directory. **Breaking:** such a name is now rejected; log in again under another name.
