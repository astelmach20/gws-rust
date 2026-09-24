---
name: recipe-share-folder-with-team
description: "Share a Google Drive folder and all its contents with a list of collaborators."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-drive
---

# Share a Google Drive Folder with a Team

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`

Share a Google Drive folder and all its contents with a list of collaborators.

## Steps

1. Find the folder: `gwsr drive files list --params '{"q": "name = '\''Project X'\'' and mimeType = '\''application/vnd.google-apps.folder'\''"}'`
2. Share as editor: `gwsr drive permissions create --params '{"fileId": "FOLDER_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "colleague@company.com"}'`
3. Share as viewer: `gwsr drive permissions create --params '{"fileId": "FOLDER_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "stakeholder@company.com"}'`
4. Verify permissions: `gwsr drive permissions list --params '{"fileId": "FOLDER_ID"}' --format table`

