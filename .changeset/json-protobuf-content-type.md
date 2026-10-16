---
"gws-rust": patch
---

Generated commands parse an `application/json+protobuf` response (Google's JSON encoding of protobuf messages) as JSON again, in any letter case. Since the exact JSON content-type check it was refused as binary and needed `-o`. Other `+json` types, such as a Drive `application/vnd.google-apps.script+json` export, are still returned as text.
