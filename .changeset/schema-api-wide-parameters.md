---
"gws-rust": patch
---

`gwsr schema <service>.<resource>.<method>` now lists the parameters that every method of the API accepts in `--params` (`fields`, `quotaUser`, `prettyPrint`, ...) under a new `apiParameters` key. Before, schema showed only the method's own parameters, although a rejected `--params` points to it for the accepted ones.
