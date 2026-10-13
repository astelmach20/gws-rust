---
"gws-rust": patch
---

`script +pull` refuses Apps Script file names containing characters that are invalid in Windows file names (`<>:"|?*`); on Windows a name such as `C:evil` was a drive-relative path that wrote outside `--dir`.
