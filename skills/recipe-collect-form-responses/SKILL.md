---
name: recipe-collect-form-responses
description: "Retrieve and review responses from a Google Form."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-forms
---

# Check Form Responses

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-forms`

Retrieve and review responses from a Google Form.

## Steps

1. List forms: `gwsr forms forms list` (if you don't have the form ID)
2. Get form details: `gwsr forms forms get --params '{"formId": "FORM_ID"}'`
3. Get responses: `gwsr forms forms responses list --params '{"formId": "FORM_ID"}' --format table`

