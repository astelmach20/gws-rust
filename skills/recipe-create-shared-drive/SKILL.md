---
name: recipe-create-shared-drive
description: "Create a Google Shared Drive and add members with appropriate roles."
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
<!-- gwsr generated skill: do not edit by hand -->

# Create and Configure a Shared Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-drive`

Create a Google Shared Drive and add members with appropriate roles.

## Steps

1. Create shared drive: `gwsr drive drives create --params '{"requestId": "unique-id-123"}' --json '{"name": "Project X"}'`
2. Add a member: `gwsr drive permissions create --params '{"fileId": "DRIVE_ID", "supportsAllDrives": true}' --json '{"role": "writer", "type": "user", "emailAddress": "member@company.com"}'`
3. List members: `gwsr drive permissions list --params '{"fileId": "DRIVE_ID", "supportsAllDrives": true}'`

