---
"gws-rust": patch
---
When `gmail +watch --cleanup` or `events +subscribe --cleanup` cannot delete the Pub/Sub resources it created, the command now exits with the class of the failure (for example 1 for a refused permission) instead of 5, and the error still names every resource it could not delete.
