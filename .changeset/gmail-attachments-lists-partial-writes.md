---
"gws-rust": patch
---
When `gmail +attachments` fails part-way, the error now lists the attachments it already wrote (or says none were written) and keeps its exit code.
