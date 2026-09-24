---
name: recipe-send-team-announcement
description: "Send a team announcement via both Gmail and a Google Chat space."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "communication"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-gmail
        - gwsr-chat
---
<!-- gwsr generated skill: do not edit by hand -->

# Announce via Gmail and Google Chat

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`, `gwsr-chat`

Send a team announcement via both Gmail and a Google Chat space.

## Steps

1. Send email: `gwsr gmail +send --to team@company.com --subject 'Important Update' --body 'Please review the attached policy changes.'`
2. Post in Chat: `gwsr chat +send --space-id spaces/TEAM_SPACE --text '📢 Important Update: Please check your email for policy changes.'`

