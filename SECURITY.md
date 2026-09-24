# Security Policy

## Supported versions

`gws-rust` (the `gwsr` binary, the `gws-rust-core` crate and the `gws-rust` npm package) is
pre-1.0. Only the **latest release** receives security fixes; upgrade before reporting.

| Version | Supported |
|---|---|
| Latest release | Yes |
| Anything older | No |
| Upstream `googleworkspace/cli` (`gws`) | No. It is unmaintained and not covered by this policy |

## Reporting a vulnerability

Report privately through GitHub's private vulnerability reporting:
**Security → Report a vulnerability** at
<https://github.com/astelmach20/gws-rust/security/advisories/new>.
Do not open a public issue, discussion or pull request for a security problem.

Please include:

- the affected version (`gwsr --version`) and install method (npm, cargo, Homebrew, archive);
- OS and architecture;
- the command, configuration and environment needed to reproduce it;
- the impact you expect (for example credential disclosure, request to a non-Google host, or
  command injection through API data).

**Never include real secrets.** Redact OAuth access and refresh tokens, client secrets and
service-account keys. If any were exposed, revoke them at
<https://myaccount.google.com/permissions> (user grants) or in the Google Cloud console
(service-account keys) before reporting.

### What to expect

| Step | Target |
|---|---|
| Acknowledgement | 3 business days |
| Initial assessment and severity | 7 days |
| Fix released and GitHub Security Advisory published (with a CVE where applicable) | 90 days, sooner for critical issues |

Disclosure is coordinated with the reporter, and reporters are credited in the advisory unless
they ask not to be.

## Scope

In scope:

- the `gwsr` binary and the `gws-rust` and `gws-rust-core` crates;
- the `gws-rust` npm package and its `gws-rust-<os>-<cpu>` platform packages;
- release artifacts, the Homebrew formula in `astelmach20/homebrew-tap`, and this repository's
  CI/CD workflows.

Out of scope:

- vulnerabilities in Google Workspace APIs themselves (report them to Google);
- attacks that require a local account with the same privileges as the user running `gwsr`;
- the generated agent skills' *content* suggesting an unsafe command, unless `gwsr` itself
  behaves unsafely when running it.

## Verifying releases

Every release is built and published only by this repository's tag-triggered release workflow
(`.github/workflows/release.yml`), running in a protected `release` environment.

- Archives carry SLSA build-provenance and CycloneDX SBOM attestations:
  `gh attestation verify gws-rust-<version>-<target>.tar.gz --repo astelmach20/gws-rust`
- `SHA256SUMS` is signed keylessly with Sigstore; verify it with
  `cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json --certificate-identity "https://github.com/astelmach20/gws-rust/.github/workflows/release.yml@refs/tags/v<version>" --certificate-oidc-issuer https://token.actions.githubusercontent.com`
- crates are published with crates.io Trusted Publishing and npm packages with npm trusted
  publishing (both OIDC; no long-lived registry tokens exist). npm packages carry npm provenance
  (`npm audit signatures`). The npm package runs no install scripts and downloads nothing.
