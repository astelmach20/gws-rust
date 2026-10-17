---
"gws-rust": patch
---

`script +pull` refuses, before writing anything, a project in which two files need the same local path: one as a file and one as a directory (`a`, saved as `a.gs`, and `a.gs/b`), or one name listed twice. Before, it wrote the files up to the clash and then failed part-way, or silently kept only the last of two duplicates.
