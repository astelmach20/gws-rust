---
name: recipe-generate-report-from-sheet
description: "Read data from a Google Sheet and create a formatted Google Docs report."
metadata:
  version: 0.24.0
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
<!-- gwsr generated skill: do not edit by hand -->

# Generate a Google Docs Report from Sheet Data

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`, `gwsr-docs`, `gwsr-drive` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Read data from a Google Sheet and create a formatted Google Docs report.

## Steps

1. Read the data: `gwsr sheets +read --spreadsheet-id SHEET_ID --range 'Sales!A1:D'`
2. Create the report doc: `gwsr docs documents create --json '{"title": "Sales Report - January 2025"}'`
3. Write the report: `printf '## Sales Report - January 2025\n### Summary\n- Total deals: 45\n- Revenue: $125,000\n### Top Deals\n1. Acme Corp - $25,000\n2. Widget Inc - $18,000\n' | gwsr docs +write --document-id DOC_ID --markdown --text-file -`
4. Share with stakeholders: `gwsr drive permissions create --params '{"fileId": "DOC_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "cfo@company.com"}'`

