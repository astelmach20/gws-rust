---
name: gwsr-chromemanagement
description: "Google Chrome Management: Chrome browser and ChromeOS device reports and telemetry."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr chromemanagement --help"
---

# chromemanagement (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr chromemanagement <resource> <method> [flags]
```

## API Resources

### customers

  - `apps` — Operations on the 'apps' resource
  - `certificateProvisioningProcesses` — Operations on the 'certificateProvisioningProcesses' resource
  - `connectorConfigs` — Operations on the 'connectorConfigs' resource
  - `enterprise` — Operations on the 'enterprise' resource
  - `profiles` — Operations on the 'profiles' resource
  - `reports` — Operations on the 'reports' resource
  - `telemetry` — Operations on the 'telemetry' resource
  - `thirdPartyProfileUsers` — Operations on the 'thirdPartyProfileUsers' resource

### operations

  - `cancel` — Starts asynchronous cancellation on a long-running operation. The server makes a best effort to cancel the operation, but success is not guaranteed. If the server doesn't support this method, it returns `google.rpc.Code.UNIMPLEMENTED`. Clients can use Operations.GetOperation or other methods to check whether the cancellation succeeded or whether the operation completed despite cancellation.
  - `delete` — Deletes a long-running operation. This method indicates that the client is no longer interested in the operation result. It does not cancel the operation. If the server doesn't support this method, it returns `google.rpc.Code.UNIMPLEMENTED`.
  - `list` — Lists operations that match the specified filter in the request. If the server doesn't support this method, it returns `UNIMPLEMENTED`.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr chromemanagement --help

# Inspect a method's required params, types, and defaults
gwsr schema chromemanagement.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

