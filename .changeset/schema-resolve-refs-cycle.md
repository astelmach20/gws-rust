---
"gws-rust": patch
---

`gwsr schema <method> --resolve-refs` no longer crashes with a stack overflow on recursive schemas. For example, `gmail.users.messages.get` (`MessagePart.parts`), `docs.documents.get` (`Tab.childTabs`) and 52 other Workspace methods used to abort. A reference back to a schema that is already being expanded is now left as `{"$ref": ...}`.
