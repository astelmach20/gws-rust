---
name: recipe-create-task-list
description: "Set up a new Google Tasks list with initial tasks."
metadata:
  version: 0.23.0
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

# Create a Task List and Add Tasks

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-tasks`

Set up a new Google Tasks list with initial tasks.

## Steps

1. Create task list: `gwsr tasks tasklists insert --json '{"title": "Q2 Goals"}'`
2. Add a task: `gwsr tasks tasks insert --params '{"tasklist": "TASKLIST_ID"}' --json '{"title": "Review Q1 metrics", "notes": "Pull data from analytics dashboard", "due": "2024-04-01T00:00:00Z"}'`
3. Add another task: `gwsr tasks tasks insert --params '{"tasklist": "TASKLIST_ID"}' --json '{"title": "Draft Q2 OKRs"}'`
4. List tasks: `gwsr tasks tasks list --params '{"tasklist": "TASKLIST_ID"}' --format table`

