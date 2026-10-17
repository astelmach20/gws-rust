---
"gws-rust": patch
---

`gwsr dev generate-skills --index PATH` links each skill relative to the index file and `--output-dir`. The links were always `../skills/<name>/SKILL.md`, which only worked for the repository's own `docs/skills.md` + `skills/` layout and broke for any other index or output location.
