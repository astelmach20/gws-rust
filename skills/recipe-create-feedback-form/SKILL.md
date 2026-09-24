---
name: recipe-create-feedback-form
description: "Create a Google Form for feedback and share it via Gmail."
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
        - gwsr-gmail
---
<!-- gwsr generated skill: do not edit by hand -->

# Create and Share a Google Form

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-forms`, `gwsr-gmail`

Create a Google Form for feedback and share it via Gmail.

## Steps

1. Create form: `gwsr forms forms create --json '{"info": {"title": "Event Feedback", "documentTitle": "Event Feedback Form"}}'`
2. Get the form URL from the response (responderUri field)
3. Email the form: `gwsr gmail +send --to attendees@company.com --subject 'Please share your feedback' --body 'Fill out the form: FORM_URL'`

