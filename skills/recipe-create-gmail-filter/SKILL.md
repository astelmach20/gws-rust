---
name: recipe-create-gmail-filter
description: "Create a Gmail filter to automatically label, star, or categorize incoming messages."
metadata:
  version: 0.22.5
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

# Create a Gmail Filter

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`

Create a Gmail filter to automatically label, star, or categorize incoming messages.

## Steps

1. List existing labels: `gwsr gmail users labels list --params '{"userId": "me"}' --format table`
2. Create a new label: `gwsr gmail users labels create --params '{"userId": "me"}' --json '{"name": "Receipts"}'`
3. Create a filter: `gwsr gmail +filter create --from receipts@example.com --add-label Receipts --archive`
4. Verify filter: `gwsr gmail +filter list`

