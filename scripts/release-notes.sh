#!/usr/bin/env bash
# Prints the install and verification section prepended to GitHub release notes.
#
#   scripts/release-notes.sh <version>
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <version>   (semver, no leading v)" >&2
  exit 2
fi
version="$1"
readonly repo="astelmach20/gws-rust"
readonly base="https://github.com/${repo}/releases/download/v${version}"

cat <<NOTES
## Install

| Method | Command |
|---|---|
| npm | \`npm install -g gws-rust@${version}\` |
| Cargo | \`cargo install gws-rust --version ${version} --locked\` |
| Homebrew | \`brew install astelmach20/tap/gws-rust\` |

Prebuilt archives are attached below as \`gws-rust-${version}-<target>.tar.gz\` (\`.zip\` on Windows).
Linux \`*-musl\` archives are fully static and run on any distribution.

\`\`\`sh
target=aarch64-apple-darwin   # or x86_64-apple-darwin, x86_64-unknown-linux-musl, ...
curl -fsSLO ${base}/gws-rust-${version}-\${target}.tar.gz
curl -fsSLO ${base}/SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS   # macOS: shasum -a 256 --check --ignore-missing SHA256SUMS
tar -xzf gws-rust-${version}-\${target}.tar.gz
install -m 0755 gws-rust-${version}-\${target}/gwsr ~/.local/bin/gwsr
\`\`\`

## Verify

Every archive has a SLSA build-provenance attestation and a CycloneDX SBOM attestation, and
\`SHA256SUMS\` is signed keylessly with Sigstore. Verify before installing:

\`\`\`sh
gh attestation verify gws-rust-${version}-\${target}.tar.gz --repo ${repo}
cosign verify-blob SHA256SUMS \\
  --bundle SHA256SUMS.sigstore.json \\
  --certificate-identity "https://github.com/${repo}/.github/workflows/release.yml@refs/tags/v${version}" \\
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
\`\`\`

The SBOM for this release is \`gws-rust-${version}.cdx.json\`. npm packages carry npm provenance
(\`npm audit signatures\`), and crates are published from this workflow via crates.io Trusted Publishing.

NOTES
