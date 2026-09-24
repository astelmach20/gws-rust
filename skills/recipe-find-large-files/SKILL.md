---
name: recipe-find-large-files
description: "Identify large Google Drive files consuming storage quota."
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

# Find Largest Files in Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`

Identify large Google Drive files consuming storage quota.

## Steps

1. List files sorted by size: `gwsr drive files list --params '{"orderBy": "quotaBytesUsed desc", "pageSize": 20, "fields": "files(id,name,size,mimeType,owners)"}' --format table`
2. Review the output and identify files to archive or move

