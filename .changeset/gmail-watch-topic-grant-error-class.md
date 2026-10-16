---
"gws-rust": patch
---
When `gmail +watch` cannot grant Gmail publish permission on the topic it created, the command now exits with the real error class (for example 1 for a refused permission, 10 for a lost connection) instead of 5, and the message still contains the `gcloud` command for granting the permission by hand.
