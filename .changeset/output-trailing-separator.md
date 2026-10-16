---
"gws-rust": patch
---

`-o/--output` refuses a path that ends in a separator, `.` or `..` (`-o downloads/`, `-o downloads/.`) before any request, like an existing directory (exit 3). When the directory did not exist yet, the trailing separator was dropped and the payload was saved as a file named `downloads`.
