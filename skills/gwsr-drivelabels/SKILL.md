---
name: gwsr-drivelabels
description: "Google Drive Labels: Manage Drive labels and classification."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr drivelabels --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drivelabels (v2)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr drivelabels <resource> <method> [flags]
```

## API Resources

### labels

  - `create` — Creates a label. For more information, see [Create and publish a label](https://developers.google.com/workspace/drive/labels/guides/create-label).
  - `delete` — Permanently deletes a label and related metadata on Drive items. For more information, see [Disable, enable, and delete a label](https://developers.google.com/workspace/drive/labels/guides/disable-delete-label). Once deleted, the label and related Drive item metadata will be deleted. Only draft labels and disabled labels may be deleted.
  - `delta` — Updates a single label by applying a set of update requests resulting in a new draft revision. For more information, see [Update a label](https://developers.google.com/workspace/drive/labels/guides/update-label). The batch update is all-or-nothing: If any of the update requests are invalid, no changes are applied. The resulting draft revision must be published before the changes may be used with Drive items.
  - `disable` — Disable a published label. For more information, see [Disable, enable, and delete a label](https://developers.google.com/workspace/drive/labels/guides/disable-delete-label). Disabling a label will result in a new disabled published revision based on the current published revision. If there's a draft revision, a new disabled draft revision will be created based on the latest draft revision. Older draft revisions will be deleted. Once disabled, a label may be deleted with `DeleteLabel`.
  - `enable` — Enable a disabled label and restore it to its published state. For more information, see [Disable, enable, and delete a label](https://developers.google.com/workspace/drive/labels/guides/disable-delete-label). This will result in a new published revision based on the current disabled published revision. If there's an existing disabled draft revision, a new revision will be created based on that draft and will be enabled.
  - `get` — Get a label by its resource name. For more information, see [Search for labels](https://developers.google.com/workspace/drive/labels/guides/search-label). Resource name may be any of: * `labels/{id}` - See `labels/{id}@latest` * `labels/{id}@latest` - Gets the latest revision of the label. * `labels/{id}@published` - Gets the current published revision of the label. * `labels/{id}@{revision_id}` - Gets the label at the specified revision ID.
  - `list` — List labels. For more information, see [Search for labels](https://developers.google.com/workspace/drive/labels/guides/search-label).
  - `publish` — Publish all draft changes to the label. Once published, the label may not return to its draft state. For more information, see [Create and publish a label](https://developers.google.com/workspace/drive/labels/guides/create-label). Publishing a label will result in a new published revision. All previous draft revisions will be deleted. Previous published revisions will be kept but are subject to automated deletion as needed.
  - `updateLabelCopyMode` — Updates a label's `CopyMode`. Changes to this policy aren't revisioned, don't require publishing, and take effect immediately.
  - `updateLabelEnabledAppSettings` — Updates a label's `EnabledAppSettings`. Enabling a label in a Google Workspace app allows it to be used in that app. This change isn't revisioned, doesn't require publishing, and takes effect immediately.
  - `updatePermissions` — Updates a label's permissions. If a permission for the indicated principal doesn't exist, a label permission is created, otherwise the existing permission is updated. Permissions affect the label resource as a whole, aren't revisioned, and don't require publishing.
  - `locks` — Operations on the 'locks' resource
  - `permissions` — Operations on the 'permissions' resource
  - `revisions` — Operations on the 'revisions' resource

### limits

  - `getLabel` — Get the constraints on the structure of a label; such as, the maximum number of fields allowed and maximum length of the label title.

### users

  - `getCapabilities` — Gets the user capabilities.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr drivelabels --help

# Inspect a method's required params, types, and defaults
gwsr schema drivelabels.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

