---
"gws-rust": patch
---

`drive +download` on a Google-native file no longer always points at `+export`. A shortcut now names its target (`+download --file-id TARGET`). A type that cannot be exported (Forms, Sites, folders and so on) is reported as such. Exportable types list their valid `--to` formats.
