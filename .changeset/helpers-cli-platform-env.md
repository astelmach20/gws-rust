---
"gws-rust": patch
---

Tests: helper CLI tests keep `SystemRoot` and the TLS certificate variables, so tests whose mock request must succeed pass on Windows and in the Nix build.
