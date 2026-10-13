---
"gws-rust": patch
---

`gwsr schema` keeps a dotted API version whole (`gwsr schema dfareporting:v3.4.ads.list`) instead of splitting it into a version `v3` and a resource `4`, so the schema path printed in method help works for such APIs.
