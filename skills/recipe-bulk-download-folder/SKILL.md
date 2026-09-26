---
name: recipe-bulk-download-folder
description: "List and download all files from a Google Drive folder."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-drive
---
<!-- gwsr generated skill: do not edit by hand -->

# Bulk Download Drive Folder

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`

List and download all files from a Google Drive folder.

## Steps

1. List files in folder: `gwsr drive files list --params '{"q": "'\''FOLDER_ID'\'' in parents"}' --format json`
2. Download each file: `gwsr drive files get --params '{"fileId": "FILE_ID", "alt": "media"}' -o filename.ext`
3. Export Google Docs as PDF: `gwsr drive files export --params '{"fileId": "FILE_ID", "mimeType": "application/pdf"}' -o document.pdf`

