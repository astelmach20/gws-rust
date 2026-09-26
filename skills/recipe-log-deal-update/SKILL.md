---
name: recipe-log-deal-update
description: "Append a deal status update to a Google Sheets sales tracking spreadsheet."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "sales"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-sheets
        - gwsr-drive
---
<!-- gwsr generated skill: do not edit by hand -->

# Log Deal Update to Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`, `gwsr-drive` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Append a deal status update to a Google Sheets sales tracking spreadsheet.

## Steps

1. Find the tracking sheet: `gwsr drive files list --params '{"q": "name = '\''Sales Pipeline'\'' and mimeType = '\''application/vnd.google-apps.spreadsheet'\''"}'`
2. Read current data: `gwsr sheets +read --spreadsheet-id SHEET_ID --range "Pipeline!A1:F"`
3. Append new row: `gwsr sheets +append --spreadsheet-id SHEET_ID --range 'Pipeline' --json-values '["2024-03-15", "Acme Corp", "Proposal Sent", "$50,000", "Q2", "jdoe"]'`

