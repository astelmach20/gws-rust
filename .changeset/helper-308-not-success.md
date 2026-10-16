---
"gws-rust": patch
---

Helper commands now treat an HTTP `308` that cannot be followed (one without a `Location` header) as an error, not a success. Before, a listing page answered with one ended the listing early as if it were complete. A download answered with one saved the `308` body as the file. Any other request answered with one reported success with an empty result.
