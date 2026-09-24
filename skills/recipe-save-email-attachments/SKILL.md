---
name: recipe-save-email-attachments
description: "Find Gmail messages with attachments and save them to a Google Drive folder."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-gmail
        - gwsr-drive
---

# Save Gmail Attachments to Google Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-gmail`, `gwsr-drive`

Find Gmail messages with attachments and save them to a Google Drive folder.

## Steps

1. Search for emails with attachments: `gwsr gmail +search --query 'has:attachment from:client@example.com' --format table`
2. Download the attachments: `gwsr gmail +attachments --message-id MESSAGE_ID --output-dir ./attachments`
3. Upload to Drive folder: `gwsr drive +upload --file ./attachments/report.pdf --folder-id FOLDER_ID`

