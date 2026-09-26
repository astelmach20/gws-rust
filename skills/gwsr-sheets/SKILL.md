---
name: gwsr-sheets
description: "Google Sheets: Read and write spreadsheets."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr sheets --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# sheets (v4)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr sheets <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+append`](../gwsr-sheets-append/SKILL.md) | Append rows after the last row of a table |
| [`+read`](../gwsr-sheets-read/SKILL.md) | Read values from a range, optionally exporting CSV |
| [`+write`](../gwsr-sheets-write/SKILL.md) | Overwrite values in a range (values.update) |
| [`+clear`](../gwsr-sheets-clear/SKILL.md) | Clear all values in a range (formatting is kept) |
| [`+create`](../gwsr-sheets-create/SKILL.md) | Create a new spreadsheet |

## API Resources

### spreadsheets

  - `batchUpdate` — Applies one or more updates to the spreadsheet. Each request is validated before being applied. If any request is not valid then the entire request will fail and nothing will be applied. Some requests have replies to give you some information about how they are applied. The replies will mirror the requests. For example, if you applied 4 updates and the 3rd one had a reply, then the response will have 2 empty replies, the actual reply, and another empty reply, in that order.
  - `create` — Creates a spreadsheet, returning the newly created spreadsheet.
  - `get` — Returns the spreadsheet at the given ID. The caller must specify the spreadsheet ID. By default, data within grids is not returned. You can include grid data in one of 2 ways: * Specify a [field mask](https://developers.google.com/workspace/sheets/api/guides/field-masks) listing your desired fields using the `fields` URL parameter in HTTP * Set the includeGridData URL parameter to true.
  - `getByDataFilter` — Returns the spreadsheet at the given ID. The caller must specify the spreadsheet ID. For more information, see [Read, write, and search metadata](https://developers.google.com/workspace/sheets/api/guides/metadata). This method differs from GetSpreadsheet in that it allows selecting which subsets of spreadsheet data to return by specifying a dataFilters parameter. Multiple DataFilters can be specified.
  - `developerMetadata` — Operations on the 'developerMetadata' resource
  - `sheets` — Operations on the 'sheets' resource
  - `values` — Operations on the 'values' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr sheets --help

# Inspect a method's required params, types, and defaults
gwsr schema sheets.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

