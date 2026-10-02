---
name: recipe-organize-drive-folder
description: "Create a Google Drive folder structure and move files into the right locations."
metadata:
  version: 0.23.1
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

# Organize Files into Google Drive Folders

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Create a Google Drive folder structure and move files into the right locations.

## Steps

1. Create a project folder: `gwsr drive files create --json '{"name": "Q2 Project", "mimeType": "application/vnd.google-apps.folder"}'`
2. Create sub-folders: `gwsr drive files create --json '{"name": "Documents", "mimeType": "application/vnd.google-apps.folder", "parents": ["PARENT_FOLDER_ID"]}'`
3. Move existing files into folder: `gwsr drive files update --params '{"fileId": "FILE_ID", "addParents": "FOLDER_ID", "removeParents": "OLD_PARENT_ID"}'`
4. Verify structure: `gwsr drive files list --params '{"q": "FOLDER_ID in parents"}' --format table`

