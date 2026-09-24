---
name: recipe-compare-sheet-tabs
description: "Read data from two tabs in a Google Sheet to compare and identify differences."
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
---

# Compare Two Google Sheets Tabs

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-sheets`

Read data from two tabs in a Google Sheet to compare and identify differences.

## Steps

1. Read the first tab: `gwsr sheets +read --spreadsheet SHEET_ID --range "January!A1:D"`
2. Read the second tab: `gwsr sheets +read --spreadsheet SHEET_ID --range "February!A1:D"`
3. Compare the data and identify changes

