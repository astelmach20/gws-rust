---
"gws-rust": patch
---

`gwsr batch` reads a batch response whose Content-Type names the multipart boundary as `Boundary=` (or any other capitalization, with or without quotes and spaces). MIME parameter names are case-insensitive. Before, such a response failed as "no multipart boundary" after the calls had been applied, and none of their results were printed.
