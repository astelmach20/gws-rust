---
name: gwsr-gmail
description: "Gmail: Send, read, and manage email."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail --help"
---

# gmail (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr gmail <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+send`](../gwsr-gmail-send/SKILL.md) | Send an email |
| [`+reply`](../gwsr-gmail-reply/SKILL.md) | Reply to a message (handles threading automatically) |
| [`+reply-all`](../gwsr-gmail-reply-all/SKILL.md) | Reply to all recipients of a message (handles threading automatically) |
| [`+forward`](../gwsr-gmail-forward/SKILL.md) | Forward a message to new recipients |
| [`+read`](../gwsr-gmail-read/SKILL.md) | Read a message and print its body and optionally headers |
| [`+triage`](../gwsr-gmail-triage/SKILL.md) | Show an unread inbox summary (sender, subject, date) |
| [`+search`](../gwsr-gmail-search/SKILL.md) | Search messages and return full metadata, with pagination |
| [`+label`](../gwsr-gmail-label/SKILL.md) | Add or remove labels on messages or threads |
| [`+archive`](../gwsr-gmail-archive/SKILL.md) | Archive messages or threads (remove from Inbox) |
| [`+trash`](../gwsr-gmail-trash/SKILL.md) | Move messages or threads to Trash |
| [`+filter`](../gwsr-gmail-filter/SKILL.md) | List, create, or delete Gmail filters |
| [`+unsubscribe`](../gwsr-gmail-unsubscribe/SKILL.md) | Unsubscribe from a mailing list via RFC 8058 one-click (List-Unsubscribe) |
| [`+resolve-url`](../gwsr-gmail-resolve-url/SKILL.md) | Resolve a Gmail web URL (or its FMfcg... token) to an API thread or message ID |
| [`+attachments`](../gwsr-gmail-attachments/SKILL.md) | Download a message's attachments as decoded files |
| [`+watch`](../gwsr-gmail-watch/SKILL.md) | Watch for new emails and stream them as NDJSON |

## API Resources

### users

  - `getProfile` — Gets the current user's Gmail profile.
  - `stop` — Turn off push notification delivery for the given user mailbox. For more information, see [Configure push notifications in Gmail API](https://developers.google.com/workspace/gmail/api/guides/push).
  - `watch` — Set up or update a push notification watch on the given user mailbox. For more information, see [Configure push notifications in Gmail API](https://developers.google.com/workspace/gmail/api/guides/push).
  - `drafts` — Operations on the 'drafts' resource
  - `history` — Operations on the 'history' resource
  - `labels` — Operations on the 'labels' resource
  - `messages` — Operations on the 'messages' resource
  - `settings` — Operations on the 'settings' resource
  - `threads` — Operations on the 'threads' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr gmail --help

# Inspect a method's required params, types, and defaults
gwsr schema gmail.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

