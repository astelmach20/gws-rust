---
"gws-rust": patch
---

npm: the `gws-rust` package and every platform package (`gws-rust-<os>-<cpu>`) now include the `NOTICE` file, as the Apache-2.0 license requires for redistribution. Before, the package build copied `NOTICE` into the main package but its `files` list left it out of the published tarball, and the platform packages, which carry the binary, never had it.
