---
name: recipe-label-and-archive-emails
description: "Apply Gmail labels to matching messages and archive them to keep your inbox clean."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-gmail
---
<!-- gwsr generated skill: do not edit by hand -->

# Label and Archive Gmail Threads

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Apply Gmail labels to matching messages and archive them to keep your inbox clean.

## Steps

1. Search for matching emails: `gwsr gmail +search --query 'from:notifications@service.com' --format table`
2. Apply a label (by name; it must already exist): `gwsr gmail +label --message-id MESSAGE_ID --add Notifications`
3. Archive (remove from inbox): `gwsr gmail +archive --message-id MESSAGE_ID`

