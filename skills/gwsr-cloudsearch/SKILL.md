---
name: gwsr-cloudsearch
description: "Google Cloud Search: Manage Cloud Search data sources and queries."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr cloudsearch --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# cloudsearch (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr cloudsearch <resource> <method> [flags]
```

## API Resources

### debug

  - `datasources` — Operations on the 'datasources' resource
  - `identitysources` — Operations on the 'identitysources' resource

### indexing

  - `datasources` — Operations on the 'datasources' resource

### media

  - `upload` — Uploads media for indexing. The upload endpoint supports direct and resumable upload protocols and is intended for large items that can not be [inlined during index requests](https://developers.google.com/workspace/cloud-search/docs/reference/rest/v1/indexing.datasources.items#itemcontent). To index large content: 1. Call indexing.datasources.items.upload with the item name to begin an upload session and retrieve the UploadItemRef. 1.

### operations

  - `get` — Gets the latest state of a long-running operation. Clients can use this method to poll the operation result at intervals as recommended by the API service.
  - `lro` — Operations on the 'lro' resource

### query

  - `removeActivity` — Provides functionality to remove logged activity for a user. Currently to be used only for Chat 1p clients **Note:** This API requires a standard end user account to execute. A service account can't perform Remove Activity requests directly; to use a service account to perform queries, set up [Google Workspace domain-wide delegation of authority](https://developers.google.com/workspace/cloud-search/docs/guides/delegation/).
  - `search` — The Cloud Search Query API provides the search method, which returns the most relevant results from a user query. The results can come from Google Workspace apps, such as Gmail or Google Drive, or they can come from data that you have indexed from a third party. **Note:** This API requires a standard end user account to execute.
  - `suggest` — Provides suggestions for autocompleting the query. **Note:** This API requires a standard end user account to execute. A service account can't perform Query API requests directly; to use a service account to perform queries, set up [Google Workspace domain-wide delegation of authority](https://developers.google.com/workspace/cloud-search/docs/guides/delegation/).
  - `sources` — Operations on the 'sources' resource

### settings

  - `getCustomer` — Get customer settings. **Note:** This API requires an admin account to execute.
  - `updateCustomer` — Update customer settings. **Note:** This API requires an admin account to execute.
  - `datasources` — Operations on the 'datasources' resource
  - `searchapplications` — Operations on the 'searchapplications' resource

### stats

  - `getIndex` — Gets indexed item statistics aggreggated across all data sources. This API only returns statistics for previous dates; it doesn't return statistics for the current day. **Note:** This API requires a standard end user account to execute.
  - `getQuery` — Get the query statistics for customer. **Note:** This API requires a standard end user account to execute.
  - `getSearchapplication` — Get search application stats for customer. **Note:** This API requires a standard end user account to execute.
  - `getSession` — Get the # of search sessions, % of successful sessions with a click query statistics for customer. **Note:** This API requires a standard end user account to execute.
  - `getUser` — Get the users statistics for customer. **Note:** This API requires a standard end user account to execute.
  - `index` — Operations on the 'index' resource
  - `query` — Operations on the 'query' resource
  - `session` — Operations on the 'session' resource
  - `user` — Operations on the 'user' resource

### v1

  - `initializeCustomer` — Enables `third party` support in Google Cloud Search. **Note:** This API requires an admin account to execute.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr cloudsearch --help

# Inspect a method's required params, types, and defaults
gwsr schema cloudsearch.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

