---
"gws-rust": patch
---

`gmail +reply` to a message you sent now goes to that message's original To recipients, as in Gmail, instead of back to yourself. A message counts as yours when its From matches your primary address or the `--from` alias, and Reply-To is ignored in that case. Replying to a note you sent only to yourself still goes to you. `+reply` now also reads your Gmail profile address to detect this.
