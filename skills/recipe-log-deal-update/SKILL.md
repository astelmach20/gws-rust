---
name: recipe-log-deal-update
description: "Append a deal status update to a Google Sheets sales tracking spreadsheet."
metadata:
  version: 0.22.5
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

# Log Deal Update to Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`, `gwsr-drive`

Append a deal status update to a Google Sheets sales tracking spreadsheet.

## Steps

1. Find the tracking sheet: `gwsr drive files list --params '{"q": "name = '\''Sales Pipeline'\'' and mimeType = '\''application/vnd.google-apps.spreadsheet'\''"}'`
2. Read current data: `gwsr sheets +read --spreadsheet SHEET_ID --range "Pipeline!A1:F"`
3. Append new row: `gwsr sheets +append --spreadsheet SHEET_ID --range 'Pipeline' --values '["2024-03-15", "Acme Corp", "Proposal Sent", "$50,000", "Q2", "jdoe"]'`

