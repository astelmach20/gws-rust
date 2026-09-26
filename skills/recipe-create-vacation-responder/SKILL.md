---
name: recipe-create-vacation-responder
description: "Enable a Gmail out-of-office auto-reply with a custom message and date range."
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

# Set Up a Gmail Vacation Responder

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`

Enable a Gmail out-of-office auto-reply with a custom message and date range.

## Steps

1. Enable vacation responder: `gwsr gmail users settings updateVacation --params '{"userId": "me"}' --json '{"enableAutoReply": true, "responseSubject": "Out of Office", "responseBodyPlainText": "I am out of the office until Jan 20. For urgent matters, contact backup@company.com.", "restrictToContacts": false, "restrictToDomain": false}'`
2. Verify settings: `gwsr gmail users settings getVacation --params '{"userId": "me"}'`
3. Disable when back: `gwsr gmail users settings updateVacation --params '{"userId": "me"}' --json '{"enableAutoReply": false}'`

