---
"gws-rust": patch
---

Discovery methods declared at the top level of a document, outside any resource (for example `oauth2:v2`'s `tokeninfo`), are now commands (`gwsr oauth2:v2 tokeninfo`), schema entries (`gwsr schema oauth2:v2.tokeninfo`) and `gwsr batch` targets. They used to be dropped without any message. Their paths go through the same endpoint-safety checks as resource methods.
