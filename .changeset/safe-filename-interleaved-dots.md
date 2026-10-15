---
"gws-rust": patch
---

Local file names derived from remote names (`drive +download`, `drive +sync` and `gmail +attachments`) are no longer left hidden or with a trailing dot when dots and whitespace are mixed at either end. A remote name such as `" .bashrc"` used to be saved as the hidden file `.bashrc`, and `"report.<no-break space>."` as `report.`, a name Windows silently changes. Dots and whitespace are now trimmed together.
