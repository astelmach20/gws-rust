---
"gws-rust": patch
---

`script +push` takes only `appsscript.json` as the manifest. A local `appsscript.gs`, `appsscript.js` or `appsscript.html` used to count as the manifest, so the push went out with no manifest and the API rejected it with an unhelpful error. It is now refused before anything is sent, with an error that names the file and asks for `appsscript.json`.
