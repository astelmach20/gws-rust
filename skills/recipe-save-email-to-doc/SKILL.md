---
name: recipe-save-email-to-doc
description: "Save a Gmail message body into a Google Doc for archival or reference."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-gmail
        - gwsr-docs
---
<!-- gwsr generated skill: do not edit by hand -->

# Save a Gmail Message to Google Docs

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`, `gwsr-docs` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Save a Gmail message body into a Google Doc for archival or reference.

## Steps

1. Find the message: `gwsr gmail users messages list --params '{"userId": "me", "q": "subject:important from:boss@company.com"}' --format table`
2. Get message content: `gwsr gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'`
3. Create a doc with the content: `gwsr docs documents create --json '{"title": "Saved Email - Important Update"}'`
4. Write the email body: `gwsr docs +write --document-id DOC_ID --text 'From: boss@company.com
Subject: Important Update

[EMAIL BODY]'`

