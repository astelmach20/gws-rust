---
"gws-rust": patch
---

npm: stop publishing a Windows platform package for now (npm blocks the `gws-rust-win32-x64` name). On Windows, install from the `.zip` release archive or with `cargo install gws-rust`; the npm launcher now points there.
