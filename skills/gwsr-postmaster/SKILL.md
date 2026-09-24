---
name: gwsr-postmaster
description: "Gmail Postmaster Tools: Gmail sender reputation and delivery metrics (Postmaster Tools)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr postmaster --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# postmaster (v2)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr postmaster <resource> <method> [flags]
```

## API Resources

### domainStats

  - `batchQuery` — Executes a batch of QueryDomainStats requests for multiple domains. Returns PERMISSION_DENIED if you don't have permission to access DomainStats for any of the requested domains.

### domains

  - `create` — [Developer Preview](https://developers.google.com/workspace/preview): Adds a domain to the user's account. Returns INVALID_ARGUMENT if a domain is not provided. Returns ALREADY_EXISTS if the domain is already registered by the user.
  - `delete` — [Developer Preview](https://developers.google.com/workspace/preview): Deletes a domain from the user's account. Returns NOT_FOUND if the domain is not registered by the user.
  - `get` — Retrieves detailed information about a domain registered by you. Returns NOT_FOUND if the domain is not registered by you. Domain represents the metadata of a domain that has been registered within the system and linked to a user.
  - `getComplianceStatus` — Retrieves the compliance status for a given domain. Returns PERMISSION_DENIED if you don't have permission to access compliance status for the domain.
  - `getVerificationToken` — [Developer Preview](https://developers.google.com/workspace/preview): Gets a verification token used for verifying a user's ownership over a domain.
  - `list` — Retrieves a list of all domains registered by you, along with their corresponding metadata. The order of domains in the response is unspecified and non-deterministic. Newly registered domains will not necessarily be added to the end of this list.
  - `verify` — [Developer Preview](https://developers.google.com/workspace/preview): Verifies a user's ownership of a domain at the DNS level. Note that this is distinct from checking if the user has OWNER status within IRDB.
  - `domainStats` — Operations on the 'domainStats' resource
  - `users` — Operations on the 'users' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr postmaster --help

# Inspect a method's required params, types, and defaults
gwsr schema postmaster.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

