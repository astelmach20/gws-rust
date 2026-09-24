---
name: recipe-generate-report-from-sheet
description: "Read data from a Google Sheet and create a formatted Google Docs report."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-sheets
        - gwsr-docs
        - gwsr-drive
---

# Generate a Google Docs Report from Sheet Data

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`, `gwsr-docs`, `gwsr-drive`

Read data from a Google Sheet and create a formatted Google Docs report.

## Steps

1. Read the data: `gwsr sheets +read --spreadsheet-id SHEET_ID --range "Sales!A1:D"`
2. Create the report doc: `gwsr docs documents create --json '{"title": "Sales Report - January 2025"}'`
3. Write the report: `gwsr docs +write --document-id DOC_ID --text '## Sales Report - January 2025

### Summary
Total deals: 45
Revenue: $125,000

### Top Deals
1. Acme Corp - $25,000
2. Widget Inc - $18,000'`
4. Share with stakeholders: `gwsr drive permissions create --params '{"fileId": "DOC_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "cfo@company.com"}'`

