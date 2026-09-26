---
"gws-rust": patch
---

Tests no longer share one HTTP connection pool across Tokio runtimes. Each `#[tokio::test]` runs its own runtime, and a pooled connection is driven by the runtime that opened it, so wiremock-backed tests (Pub/Sub pull, Workspace Events renew, Calendar, Docs) intermittently failed with "dispatch task is gone: runtime dropped the dispatch task". The CLI itself runs one runtime and keeps its single shared client; `gws_rust_core::client::build_client()` builds an unshared client for callers that run more than one runtime.
