---
name: recipe-post-mortem-setup
description: "Create a Google Docs post-mortem, schedule a Google Calendar review, and notify via Chat."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "engineering"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-docs
        - gwsr-calendar
        - gwsr-chat
---

# Set Up Post-Mortem

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-docs`, `gwsr-calendar`, `gwsr-chat`

Create a Google Docs post-mortem, schedule a Google Calendar review, and notify via Chat.

## Steps

1. Create post-mortem doc: `gwsr docs +write --title 'Post-Mortem: [Incident]' --body '## Summary\n\n## Timeline\n\n## Root Cause\n\n## Action Items'`
2. Schedule review meeting: `gwsr calendar +insert --summary 'Post-Mortem Review: [Incident]' --attendee team@company.com --start '2026-03-16T14:00:00' --end '2026-03-16T15:00:00'`
3. Notify in Chat: `gwsr chat +send --space spaces/ENG_SPACE --text '🔍 Post-mortem scheduled for [Incident].'`

