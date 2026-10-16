---
"gws-rust": patch
---

`docs +read` now reads every tab of a multi-tab document, child tabs included. It requests `includeTabsContent=true`; before, the Docs API returned only the first tab and the other tabs' text was silently dropped. With more than one tab, the text and Markdown output start each tab with a `=== Tab: Parent > Child ===` line, and the JSON output gains a `tabs` list (`tabId`, `title`, `parentTabId`). `--body-format raw` now prints the document with its content under `tabs` rather than `body`. The `+write` and `+replace` help now states their tab behavior: `+write` appends to the first tab, and `+replace` replaces in every tab.
