# gws-rust

**One CLI for all of Google Workspace — built for humans and AI agents.**

`gwsr` dynamically generates its command surface at runtime by reading Google's [Discovery Service](https://developers.google.com/discovery). Drive, Gmail, Calendar, and every Workspace API — zero boilerplate, structured JSON output, 40+ agent skills included.

## Install

Download the pre-built binary for your OS and architecture from the **[GitHub Releases](https://github.com/astelmach20/gws-rust/releases)** page.

Alternatively, you can use package managers as a convenience layer:

```bash
npm install -g gws-rust    # npm (downloads GitHub release binary)
cargo install gws-rust     # crates.io
nix run github:astelmach20/gws-rust     # nix
```

## Quick Start

```bash
gwsr auth login
gwsr drive files list --params '{"pageSize": 5}'
gwsr gmail users.messages list --params '{"maxResults": 3}'
```

## Documentation

See the [full README](https://github.com/astelmach20/gws-rust#readme) for authentication setup, helper commands, agent skills, and more.

## License

Apache-2.0 — see [LICENSE](https://github.com/astelmach20/gws-rust/blob/main/LICENSE).
