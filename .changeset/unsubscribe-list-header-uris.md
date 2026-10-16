---
"gws-rust": patch
---

`gmail +unsubscribe` now reads the `List-Unsubscribe` URIs from their angle brackets, as RFC 2369 defines them. A URI containing a comma (`?ids=1,2`) was cut at the comma, and an entry followed by a comment (`<https://…> (unsubscribe)`) kept the comment, so the one-click POST went to the wrong URL.
