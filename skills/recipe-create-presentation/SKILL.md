---
name: recipe-create-presentation
description: "Create a new Google Slides presentation and add initial slides."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-slides
---
<!-- gwsr generated skill: do not edit by hand -->

# Create a Google Slides Presentation

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-slides`

Create a new Google Slides presentation and add initial slides.

## Steps

1. Create presentation: `gwsr slides presentations create --json '{"title": "Quarterly Review Q2"}'`
2. Get the presentation ID from the response
3. Share with team: `gwsr drive permissions create --params '{"fileId": "PRESENTATION_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "team@company.com"}'`

