---
name: recipe-create-doc-from-template
description: "Copy a Google Docs template, fill in content, and share with collaborators."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-drive
        - gwsr-docs
---
<!-- gwsr generated skill: do not edit by hand -->

# Create a Google Doc from a Template

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`, `gwsr-docs`

Copy a Google Docs template, fill in content, and share with collaborators.

## Steps

1. Copy the template: `gwsr drive files copy --params '{"fileId": "TEMPLATE_DOC_ID"}' --json '{"name": "Project Brief - Q2 Launch"}'`
2. Get the new doc ID from the response
3. Add content: `gwsr docs +write --document-id NEW_DOC_ID --text '## Project: Q2 Launch

### Objective
Launch the new feature by end of Q2.'`
4. Share with team: `gwsr drive permissions create --params '{"fileId": "NEW_DOC_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "team@company.com"}'`

