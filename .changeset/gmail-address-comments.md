---
"gws-rust": patch
---

Gmail helpers now handle RFC 5322 comments in addresses. A message whose sender uses the legacy `alice@example.com (Alice Smith)` form is replied to at `alice@example.com`, not at the invalid address `alice@example.com (Alice Smith)`. A comma inside a comment, as in `bob@example.com (Smith, Bob)`, no longer splits one recipient into two broken ones in `+reply-all` Cc lists or in `--to`/`--cc`/`--bcc`. Parentheses that are not comments, such as `Malo (Work) <malo@example.com>` or an emoticon in a display name, are kept as they are.
