---
"gws-rust": patch
---

`gwsr dev generate-skills` writes each `SKILL.md` and the `--index` file atomically, and replaces a symlink at a skill's path instead of writing through it, so a symlink in `--output-dir` can no longer overwrite the file it points to (or, with `GWSR_RESTRICT_PATHS=cwd`, reach outside the current directory: that is now refused). Every skill and the index are rendered, and every `--filter` checked, before anything is written, so a failed run (an unmatched `--filter`, a Discovery fetch error) no longer leaves some skills written.
