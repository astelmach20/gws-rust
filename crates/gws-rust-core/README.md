# gws-rust-core

The library behind [`gws-rust`](https://crates.io/crates/gws-rust) (the `gwsr` CLI). It works with Google Workspace APIs through the [Discovery Service](https://developers.google.com/discovery): it fetches Discovery documents at runtime instead of using generated per-API client crates.

## Modules

| Module | Description |
|---|---|
| `discovery` | Discovery document types (`RestDescription`, `RestResource`, `RestMethod`, ...) and `DiscoveryLoader`, which fetches, validates and optionally caches them on disk (`DiscoveryCache`: 0600 files, 24-hour TTL, stale fallback) |
| `services` | The Workspace service registry (`SERVICES`) and `resolve_service()` for aliases and `<api>:<version>` specs |
| `client` | The shared `reqwest` client, `RetryPolicy` and `send()` (exponential backoff with jitter, `Retry-After`, no re-send of non-idempotent requests) |
| `validate` | Input validation: path policy, resource names, URL path encoding, API endpoint trust, Model Armor template names |
| `error` | `GwsError` with stable exit codes and the JSON error envelope |

The `anyhow` feature adds `From<anyhow::Error> for GwsError`.

## Usage

```rust
use gws_rust_core::discovery::{DiscoveryCache, DiscoveryLoader};
use gws_rust_core::services::resolve_service;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (api, version) = resolve_service("drive")?;
    let loaded = DiscoveryLoader::new()
        .with_cache(DiscoveryCache::new(std::env::temp_dir().join("discovery")))
        .load(&api, &version)
        .await?;
    let doc = loaded.doc;
    println!("{} {}: {} resources", doc.name, doc.version, doc.resources.len());
    Ok(())
}
```

## License

Apache-2.0; see [LICENSE](https://github.com/astelmach20/gws-rust/blob/main/LICENSE) and [NOTICE](https://github.com/astelmach20/gws-rust/blob/main/NOTICE).
