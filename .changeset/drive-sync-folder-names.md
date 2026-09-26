---
"gws-rust": patch
---

`drive +sync` no longer merges sibling Drive folders that share a name, or a folder and a file with the same name, into one local path (which lost files and reported them as "unchanged"). Files and sub-folders in a Drive folder now share one set of local names: when more than one item maps to the same local name, every one of them is saved as `{id}-{name}`, so the layout no longer depends on the order Drive lists items in. This changes local paths for same-named files: previously the first one listed kept the plain name. An earlier sync's plain-named copy of such an item is left in place and is no longer updated. If an ID-prefixed name still collides with another item's real name, `+sync` fails before downloading anything from that folder instead of overwriting a file.
