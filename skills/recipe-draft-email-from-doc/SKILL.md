---
name: recipe-draft-email-from-doc
description: "Read content from a Google Doc and use it as the body of a Gmail message."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-docs
        - gwsr-gmail
---
<!-- gwsr generated skill: do not edit by hand -->

# Draft a Gmail Message from a Google Doc

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-docs`, `gwsr-gmail` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Read content from a Google Doc and use it as the body of a Gmail message.

## Steps

1. Get the document content: `gwsr docs documents get --params '{"documentId": "DOC_ID"}'`
2. Copy the text from the body content
3. Send the email: `gwsr gmail +send --to recipient@example.com --subject 'Newsletter Update' --body 'CONTENT_FROM_DOC'`

