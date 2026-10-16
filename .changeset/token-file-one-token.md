---
"gws-rust": patch
---

`GWSR_TOKEN_FILE` is now checked like `GWSR_TOKEN`: after trimming surrounding whitespace, the file must hold exactly one token with no inner whitespace, control or non-ASCII characters. Before, a file with two tokens on separate lines, a JSON token response or a pasted `Bearer ` prefix was accepted and put into the `Authorization` header as is. That failed with an unrelated request-builder or HTTP 401 error, after a pointless token "refresh". It is now a configuration error (exit 8) that names the file and does not echo its contents.
