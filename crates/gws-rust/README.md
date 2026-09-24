# gws-rust

**One CLI for every Google Workspace API, built for scripts and AI agents.**

`gwsr` builds its commands at runtime from Google's [Discovery Service](https://developers.google.com/discovery), so Drive, Gmail, Calendar, Sheets, Docs, Admin and any other Discovery API work with no per-API code. It prints JSON on stdout by default, reports errors as structured JSON on stderr with stable exit codes, and ships 151 generated agent skills.

## Install

```bash
cargo install gws-rust --locked                 # crates.io (builds from source)
npm install -g gws-rust                         # prebuilt binary via per-platform npm packages
brew install astelmach20/tap/gws-rust           # Homebrew
nix run github:astelmach20/gws-rust -- --help   # Nix
```

Signed, attested release archives are on [GitHub Releases](https://github.com/astelmach20/gws-rust/releases).

## Quick start

```bash
gwsr auth setup      # create an OAuth client with gcloud (or add client_secret.json yourself)
gwsr auth login      # read-only scopes by default; --write for read-write
gwsr drive files list --params '{"pageSize": 5}' --fields 'files(id,name)'
gwsr gmail users messages list --params '{"userId": "me", "maxResults": 3}'
gwsr calendar +agenda --today --format table
```

## Documentation

See the [full README](https://github.com/astelmach20/gws-rust#readme) for installation and verification, authentication, the output contract, helpers, configuration and agent skills.

## License

Apache-2.0; see [LICENSE](https://github.com/astelmach20/gws-rust/blob/main/LICENSE) and [NOTICE](https://github.com/astelmach20/gws-rust/blob/main/NOTICE). Derived from [googleworkspace/cli](https://github.com/googleworkspace/cli). Not an officially supported Google product.
