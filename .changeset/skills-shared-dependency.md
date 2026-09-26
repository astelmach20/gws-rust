---
"gws-rust": patch
---

Skills installed one at a time now say what they depend on. Every service and helper skill lists `gwsr-shared` under `metadata.openclaw.requires.skills` and its prerequisite line gives the `npx skills add` command for `gwsr-shared`; recipe and persona skills say how to install any listed skill that is missing. The README's selective-install example installs `gwsr-shared` first.
