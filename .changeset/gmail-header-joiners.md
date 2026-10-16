---
"gws-rust": patch
---

Gmail `+send`, `+reply`, `+reply-all` and `+forward` once again accept the zero-width joiner and non-joiner (U+200D, U+200C) in `--subject` and in display names in `--to`, `--cc`, `--bcc` and `--from`. Emoji sequences such as 👨‍👩‍👧 and Persian or Indic text use these characters, and they had been refused with exit 3. Other invisible and bidi characters are still refused, and an email address may not contain any of them, joiners included.
