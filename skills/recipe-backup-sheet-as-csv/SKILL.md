---
name: recipe-backup-sheet-as-csv
description: "Export a Google Sheets spreadsheet as a CSV file for local backup or processing."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-sheets
        - gwsr-drive
---
<!-- gwsr generated skill: do not edit by hand -->

# Export a Google Sheet as CSV

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`, `gwsr-drive`

Export a Google Sheets spreadsheet as a CSV file for local backup or processing.

## Steps

1. Get spreadsheet details: `gwsr sheets spreadsheets get --params '{"spreadsheetId": "SHEET_ID"}'`
2. Export as CSV: `gwsr drive files export --params '{"fileId": "SHEET_ID", "mimeType": "text/csv"}'`
3. Or read values directly: `gwsr sheets +read --spreadsheet-id SHEET_ID --range 'Sheet1' --format csv`

