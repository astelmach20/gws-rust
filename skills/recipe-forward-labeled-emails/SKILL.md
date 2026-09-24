---
name: recipe-forward-labeled-emails
description: "Find Gmail messages with a specific label and forward them to another address."
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

# Forward Labeled Gmail Messages

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`

Find Gmail messages with a specific label and forward them to another address.

## Steps

1. Find labeled messages: `gwsr gmail +search --query 'label:needs-review' --format table`
2. Forward a message: `gwsr gmail +forward --message-id MSG_ID --to manager@company.com --body 'Forwarding for your review.'`

