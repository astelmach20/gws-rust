---
"gws-rust": patch
---
When `gmail +watch` setup fails after it created its Pub/Sub topic or subscription (granting publish permission, creating the subscription, or `users.watch`), it now deletes the resources it created, never a `--topic` you passed, and still returns the original error.
