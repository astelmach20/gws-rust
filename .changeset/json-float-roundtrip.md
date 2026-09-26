---
"gws-rust": patch
---

Floating-point numbers in API responses are now printed exactly as the API sent them. Before, many 17-significant-digit doubles, such as coordinates or unformatted Sheets values, were read slightly wrong and printed with a changed last digit (for example `13.346133595589677` became `13.346133595589675`) in every output format.
