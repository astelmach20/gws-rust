---
name: recipe-create-meet-space
description: "Create a Google Meet meeting space and share the join link."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-meet
        - gwsr-gmail
---
<!-- gwsr generated skill: do not edit by hand -->

# Create a Google Meet Conference

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-meet`, `gwsr-gmail` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Create a Google Meet meeting space and share the join link.

## Steps

1. Create meeting space: `gwsr meet spaces create --json '{"config": {"accessType": "OPEN"}}'`
2. Copy the meeting URI from the response
3. Email the link: `gwsr gmail +send --to team@company.com --subject 'Join the meeting' --body 'Join here: MEETING_URI'`

