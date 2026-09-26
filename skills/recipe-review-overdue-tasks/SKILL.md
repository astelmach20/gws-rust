---
name: recipe-review-overdue-tasks
description: "Find Google Tasks that are past due and need attention."
metadata:
  version: 0.23.1
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-tasks
---
<!-- gwsr generated skill: do not edit by hand -->

# Review Overdue Tasks

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-tasks` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Find Google Tasks that are past due and need attention.

## Steps

1. List task lists: `gwsr tasks tasklists list --format table`
2. List tasks with status: `gwsr tasks tasks list --params '{"tasklist": "TASKLIST_ID", "showCompleted": false}' --format table`
3. Review due dates and prioritize overdue items

