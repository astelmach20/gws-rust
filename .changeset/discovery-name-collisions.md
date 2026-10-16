---
"gws-rust": patch
---

APIs whose Discovery document gives a method and a resource the same name at one level (`apigee:v1`, `aiplatform:v1`, `aiplatform:v1beta1`, `connectors:v2`, `identitytoolkit:v1`), or names one `help`, no longer crash `gwsr` while it builds the command tree. Before this fix, one of the two commands also became unreachable. Now every method has a command. The method keeps the shared name and the resource becomes `<name>-resource`, for example `gwsr apigee:v1 organizations developers attributes-resource list`. A method or resource named `help` becomes `help-method` or `help-resource`. `gwsr schema` and `gwsr batch` accept the Discovery path or the command path, and `gwsr commands` and completions list the command names.
