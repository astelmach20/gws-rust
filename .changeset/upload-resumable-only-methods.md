---
"gws-rust": patch
---
`--upload` on a method whose Discovery document advertises only the resumable protocol now uses it for small files too, instead of failing because the method has no simple upload endpoint.
