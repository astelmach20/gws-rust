---
name: recipe-share-doc-and-notify
description: "Share a Google Docs document with edit access and email collaborators the link."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-drive
        - gwsr-docs
        - gwsr-gmail
---
<!-- gwsr generated skill: do not edit by hand -->

# Share a Google Doc and Notify Collaborators

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`, `gwsr-docs`, `gwsr-gmail` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Share a Google Docs document with edit access and email collaborators the link.

## Steps

1. Find the doc: `gwsr drive files list --params '{"q": "name contains '\''Project Brief'\'' and mimeType = '\''application/vnd.google-apps.document'\''"}'`
2. Share with editor access: `gwsr drive permissions create --params '{"fileId": "DOC_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "reviewer@company.com"}'`
3. Email the link: `gwsr gmail +send --to reviewer@company.com --subject 'Please review: Project Brief' --body 'I have shared the project brief with you: https://docs.google.com/document/d/DOC_ID'`

