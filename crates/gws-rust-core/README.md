# gws-rust-core

Core Rust library for interacting with Google Workspace APIs via the [Discovery Service](https://developers.google.com/discovery).

This crate provides the foundational types and utilities used by the [`gws-rust`](https://crates.io/crates/gws-rust) (`gwsr`) command-line tool, and can be used independently for programmatic access.

> **Dynamic Discovery** — this library fetches Google's Discovery Documents at runtime rather than relying on generated client crates. When Google adds or updates an API endpoint, your code picks it up automatically.

## Modules

| Module | Description |
|---|---|
| `discovery` | Discovery Document types (`RestDescription`, `RestMethod`, etc.) and async fetch with optional disk caching |
| `services` | Service registry mapping aliases (e.g., `drive`) to API name/version pairs |
| `error` | Structured `GwsError` enum with exit codes and JSON serialization |
| `validate` | Input validation: path safety, resource name checks, URL encoding |
| `client` | HTTP client builder with automatic retry logic |

## Usage

```rust
use gws_rust_core::discovery::fetch_discovery_document;
use gws_rust_core::services::resolve_service;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (api, version) = resolve_service("drive").unwrap();
    let doc = fetch_discovery_document(api, version, None).await?;

    println!("{} {} — {} resources",
        doc.name, doc.version,
        doc.resources.len(),
    );
    Ok(())
}
```

## License

Apache-2.0 — see [LICENSE](https://github.com/astelmach20/gws-rust/blob/main/LICENSE).
