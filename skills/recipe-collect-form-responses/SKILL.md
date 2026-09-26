---
name: recipe-collect-form-responses
description: "Retrieve and review responses from a Google Form."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-forms
---
<!-- gwsr generated skill: do not edit by hand -->

# Check Form Responses

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-forms` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Retrieve and review responses from a Google Form.

## Steps

1. Find the form (the Forms API has no list method): `gwsr drive files list --params '{"q": "mimeType = '\''application/vnd.google-apps.form'\''", "fields": "files(id,name)"}'`
2. Get form details: `gwsr forms forms get --params '{"formId": "FORM_ID"}'`
3. Get responses: `gwsr forms forms responses list --params '{"formId": "FORM_ID"}' --format table`

