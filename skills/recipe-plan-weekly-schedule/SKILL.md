---
name: recipe-plan-weekly-schedule
description: "Review your Google Calendar week, identify gaps, and add events to fill them."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-calendar
---
<!-- gwsr generated skill: do not edit by hand -->

# Plan Your Weekly Google Calendar Schedule

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-calendar` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Review your Google Calendar week, identify gaps, and add events to fill them.

## Steps

1. Check this week's agenda: `gwsr calendar +agenda`
2. Check free/busy for the week: `gwsr calendar freebusy query --json '{"timeMin": "2025-01-20T00:00:00Z", "timeMax": "2025-01-25T00:00:00Z", "items": [{"id": "primary"}]}'`
3. Add a new event: `gwsr calendar +insert --summary 'Deep Work Block' --start '2026-01-21T14:00:00' --end '2026-01-21T16:00:00'`
4. Review updated schedule: `gwsr calendar +agenda`

