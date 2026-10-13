---
"gws-rust": patch
---

Treat the invisible bidi marks LRM, RLM and ALM (U+200E, U+200F, U+061C), the word joiner and invisible operators (U+2060 to U+2064) and U+180E as dangerous characters: they are now rejected in arguments and paths and stripped from terminal output, like zero-width spaces and bidi overrides already were.
