---
name: recipe-watch-drive-changes
description: "Subscribe to change notifications on a Google Drive file or folder."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "engineering"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-events
---

# Watch for Drive Changes

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-events`

Subscribe to change notifications on a Google Drive file or folder.

## Steps

1. Create subscription: `gwsr events subscriptions create --json '{"targetResource": "//drive.googleapis.com/drives/DRIVE_ID", "eventTypes": ["google.workspace.drive.file.v1.updated"], "notificationEndpoint": {"pubsubTopic": "projects/PROJECT/topics/TOPIC"}, "payloadOptions": {"includeResource": true}}'`
2. List active subscriptions: `gwsr events subscriptions list --params '{"filter": "event_types:\"google.workspace.drive.file.v1.updated\""}'`
3. Renew before expiry: `gwsr events +renew --subscription-id SUBSCRIPTION_ID --event-types google.workspace.drive.file.v1.updated`

