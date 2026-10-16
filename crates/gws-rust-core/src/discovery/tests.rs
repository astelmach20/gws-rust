// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::time::Duration;

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const DRIVE: &str = r#"{"name":"drive","version":"v3","rootUrl":"https://www.googleapis.com/","servicePath":"drive/v3/"}"#;
const PRIMARY: &str = "/discovery/v1/apis/drive/v3/rest";
const ALT: &str = "/drive/$discovery/rest";

fn loader(server: &MockServer, cache_root: Option<&std::path::Path>) -> DiscoveryLoader {
    let mut loader = DiscoveryLoader {
        discovery_base: Some(Url::parse(&format!("{}/", server.uri())).unwrap()),
        ..DiscoveryLoader::default()
    }
    .with_retry_policy(crate::client::RetryPolicy {
        max_attempts: 2,
        base_delay: Duration::ZERO,
        ..crate::client::RetryPolicy::default()
    });
    if let Some(root) = cache_root {
        loader = loader.with_cache(DiscoveryCache::new(root));
    }
    loader
}

async fn mount(server: &MockServer, p: &str, status: u16, body: &str, times: u64) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .expect(times)
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetches_validates_and_caches_then_serves_from_cache() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, DRIVE, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let loader = loader(&server, Some(tmp.path()));

    let first = loader.load("drive", "v3").await.unwrap();
    assert_eq!(first.origin, DocumentOrigin::Network);
    assert!(first.notices.is_empty());
    assert!(
        DiscoveryCache::new(tmp.path())
            .entry_path("drive", "v3")
            .exists()
    );

    let second = loader.load("drive", "v3").await.unwrap();
    assert_eq!(second.origin, DocumentOrigin::Cache);
    assert_eq!(second.doc.service_path, "drive/v3/");
}

#[tokio::test]
async fn falls_back_to_per_service_discovery_endpoint() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 404, "", 1).await;
    Mock::given(method("GET"))
        .and(path(ALT))
        .and(query_param("version", "v3"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DRIVE))
        .expect(1)
        .mount(&server)
        .await;
    let got = loader(&server, None).load("drive", "v3").await.unwrap();
    assert_eq!(got.doc.name, "drive");
}

#[tokio::test]
async fn unknown_api_is_not_found() {
    let server = MockServer::start().await;
    mount(&server, "/discovery/v1/apis/nope/v1/rest", 404, "", 1).await;
    mount(&server, "/nope/$discovery/rest", 404, "", 1).await;
    let err = loader(&server, None).load("nope", "v1").await.unwrap_err();
    assert!(matches!(err, DiscoveryError::NotFound { .. }), "{err}");
    let gws: crate::error::GwsError = err.into();
    assert_eq!(gws.exit_code(), crate::error::GwsError::EXIT_CODE_DISCOVERY);
}

#[tokio::test]
async fn captive_portal_html_is_rejected_and_not_cached() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, "<html>Sign in to Wi-Fi</html>", 1).await;
    mount(&server, ALT, 200, "<html>Sign in to Wi-Fi</html>", 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let err = loader(&server, Some(tmp.path()))
        .load("drive", "v3")
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not a Discovery Document"), "{msg}");
    assert!(
        !DiscoveryCache::new(tmp.path())
            .entry_path("drive", "v3")
            .exists()
    );
}

#[tokio::test]
async fn untrusted_root_url_from_network_is_rejected_and_not_cached() {
    let server = MockServer::start().await;
    let evil = DRIVE.replace("https://www.googleapis.com/", "http://attacker.example/");
    mount(&server, PRIMARY, 200, &evil, 1).await;
    mount(&server, ALT, 200, &evil, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let err = loader(&server, Some(tmp.path()))
        .load("drive", "v3")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("untrusted API base URL"), "{err}");
    assert!(
        !DiscoveryCache::new(tmp.path())
            .entry_path("drive", "v3")
            .exists()
    );
}

#[tokio::test]
async fn api_base_override_trusts_private_endpoint() {
    let server = MockServer::start().await;
    let private = DRIVE.replace("https://www.googleapis.com/", "https://proxy.corp.example/");
    mount(&server, PRIMARY, 200, &private, 1).await;
    let base = Url::parse("https://proxy.corp.example/").unwrap();
    let got = loader(&server, None)
        .with_api_base_override(Some(base))
        .load("drive", "v3")
        .await
        .unwrap();
    assert_eq!(got.doc.root_url, "https://proxy.corp.example/");
}

#[tokio::test]
async fn corrupt_cache_is_deleted_and_refetched() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, DRIVE, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let cache = DiscoveryCache::new(tmp.path());
    cache.store("drive", "v3", b"{ truncated").unwrap();

    let got = loader(&server, Some(tmp.path()))
        .load("drive", "v3")
        .await
        .unwrap();
    assert_eq!(got.origin, DocumentOrigin::Network);
    assert!(
        matches!(
            got.notices.as_slice(),
            [DiscoveryNotice::DiscardedCacheEntry { .. }]
        ),
        "{:?}",
        got.notices
    );
    // The refetched document replaced the corrupt entry.
    assert!(cache.load("drive", "v3", None).unwrap().is_some());
}

/// An invalid entry that cannot be deleted (here a directory where the file
/// should be) is still ignored and the document fetched again, as for an
/// entry that could not be written: the failed delete is reported, not fatal.
#[tokio::test]
async fn undeletable_invalid_cache_entry_is_reported_and_refetched() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, DRIVE, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let cache = DiscoveryCache::new(tmp.path());
    std::fs::create_dir_all(cache.entry_path("drive", "v3")).unwrap();

    let got = loader(&server, Some(tmp.path()))
        .load("drive", "v3")
        .await
        .unwrap();
    assert_eq!(got.origin, DocumentOrigin::Network);
    assert_eq!(got.doc.name, "drive");
    match got.notices.as_slice() {
        [
            DiscoveryNotice::DiscardedCacheEntry { reason, .. },
            DiscoveryNotice::CacheWriteFailed { .. },
        ] => assert!(reason.contains("could not delete"), "{reason}"),
        other => panic!("unexpected notices {other:?}"),
    }
}

#[tokio::test]
async fn tampered_cache_with_foreign_root_url_is_refetched() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, DRIVE, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    let cache = DiscoveryCache::new(tmp.path());
    let evil = DRIVE.replace("https://www.googleapis.com/", "https://attacker.example/");
    cache.store("drive", "v3", evil.as_bytes()).unwrap();

    let got = loader(&server, Some(tmp.path()))
        .load("drive", "v3")
        .await
        .unwrap();
    assert_eq!(got.doc.root_url, "https://www.googleapis.com/");
    assert_eq!(got.notices.len(), 1);
}

#[tokio::test]
async fn stale_cache_is_used_loudly_when_offline() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 503, "unavailable", 2).await;
    mount(&server, ALT, 503, "unavailable", 2).await;
    let tmp = tempfile::tempdir().unwrap();
    DiscoveryCache::new(tmp.path())
        .store("drive", "v3", DRIVE.as_bytes())
        .unwrap();
    let loader =
        loader(&server, None).with_cache(DiscoveryCache::new(tmp.path()).with_ttl(Duration::ZERO));

    let got = loader.load("drive", "v3").await.unwrap();
    assert_eq!(got.origin, DocumentOrigin::StaleCache);
    match got.notices.as_slice() {
        [notice @ DiscoveryNotice::UsingStaleCache { fetch_error, .. }] => {
            assert!(fetch_error.contains("503"), "{fetch_error}");
            assert!(notice.to_string().contains("STALE"));
        }
        other => panic!("unexpected notices {other:?}"),
    }
}

#[tokio::test]
async fn stale_cache_is_refreshed_when_online() {
    let server = MockServer::start().await;
    mount(&server, PRIMARY, 200, DRIVE, 1).await;
    let tmp = tempfile::tempdir().unwrap();
    DiscoveryCache::new(tmp.path())
        .store("drive", "v3", DRIVE.as_bytes())
        .unwrap();
    let loader =
        loader(&server, None).with_cache(DiscoveryCache::new(tmp.path()).with_ttl(Duration::ZERO));
    let got = loader.load("drive", "v3").await.unwrap();
    assert_eq!(got.origin, DocumentOrigin::Network);
}

#[tokio::test]
async fn fetch_failure_without_cache_reports_every_attempt() {
    let server = MockServer::start().await;
    // 5xx is retried (two attempts in the test policy); 404 is final.
    mount(&server, PRIMARY, 500, "", 2).await;
    mount(&server, ALT, 404, "", 1).await;
    let err = loader(&server, None).load("drive", "v3").await.unwrap_err();
    match &err {
        DiscoveryError::Fetch { attempts, .. } => {
            assert_eq!(attempts.len(), 2, "{attempts:?}");
            assert!(attempts[0].contains("500"));
            assert!(attempts[1].contains("404"));
        }
        other => panic!("unexpected {other}"),
    }
}

#[tokio::test]
async fn invalid_identifiers_never_reach_the_network() {
    let server = MockServer::start().await;
    let loader = loader(&server, None);
    for (svc, ver) in [
        ("../etc", "v1"),
        ("drive", "v3/../x"),
        ("", "v1"),
        ("a b", "v1"),
    ] {
        assert!(matches!(
            loader.load(svc, ver).await,
            Err(DiscoveryError::InvalidInput(_))
        ));
    }
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
}

#[test]
fn non_dns_label_services_skip_the_per_service_endpoint() {
    let loader = DiscoveryLoader::new();
    assert_eq!(loader.discovery_urls("drive", "v3").unwrap().len(), 2);
    let urls = loader.discovery_urls("my_api", "v1").unwrap();
    assert_eq!(urls.len(), 1);
    assert_eq!(
        urls[0].as_str(),
        "https://www.googleapis.com/discovery/v1/apis/my_api/v1/rest"
    );
    let alt = &loader.discovery_urls("forms", "v1").unwrap()[1];
    assert_eq!(
        alt.as_str(),
        "https://forms.googleapis.com/$discovery/rest?version=v1"
    );
}

#[test]
fn load_cached_is_offline_and_returns_stale_docs() {
    let tmp = tempfile::tempdir().unwrap();
    let loader =
        DiscoveryLoader::new().with_cache(DiscoveryCache::new(tmp.path()).with_ttl(Duration::ZERO));
    assert!(loader.load_cached("drive", "v3").unwrap().is_none());
    DiscoveryCache::new(tmp.path())
        .store("drive", "v3", DRIVE.as_bytes())
        .unwrap();
    assert!(loader.load_cached("drive", "v3").unwrap().is_some());
    assert!(
        DiscoveryLoader::new()
            .load_cached("drive", "v3")
            .unwrap()
            .is_none()
    );
    assert!(loader.load_cached("..", "v3").is_err());
    assert_eq!(loader.clear_cache().unwrap(), 1);
    assert_eq!(DiscoveryLoader::new().clear_cache().unwrap(), 0);
}

/// A loader for `my_api` (not a DNS label, so only the directory endpoint is
/// tried) against `base`, with the given retry policy (`None`: the default).
fn single_url_loader(base: &str, policy: Option<crate::client::RetryPolicy>) -> DiscoveryLoader {
    let loader = DiscoveryLoader {
        discovery_base: Some(Url::parse(&format!("{base}/")).unwrap()),
        ..DiscoveryLoader::default()
    };
    match policy {
        Some(policy) => loader.with_retry_policy(policy),
        None => loader,
    }
}

/// The default policy is the installed process-wide request timeout
/// (`GWSR_TIMEOUT` / `timeout_secs`), not a fixed 60 seconds. This is the only
/// test in this binary that installs it.
#[tokio::test]
async fn default_policy_uses_the_installed_request_timeout() {
    crate::client::install_request_timeout(Some(Duration::from_millis(200))).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/discovery/v1/apis/my_api/v1/rest"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"name":"my_api","version":"v1"}"#)
                .set_delay(Duration::from_secs(20)),
        )
        .mount(&server)
        .await;
    let loader = single_url_loader(&server.uri(), None);
    let result = tokio::time::timeout(Duration::from_secs(15), loader.load("my_api", "v1"))
        .await
        .expect("the installed timeout must bound the fetch");
    let err = result.unwrap_err();
    assert!(matches!(err, DiscoveryError::Fetch { .. }), "{err:?}");
}

/// A body that stalls is bounded by the policy's response timeout, not a
/// fixed 60-second total.
#[tokio::test]
async fn a_stalled_body_is_bounded_by_the_policy_timeout() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _read = sock.read(&mut buf).await.unwrap();
        sock.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{\"name\":",
        )
        .await
        .unwrap();
        sock.flush().await.unwrap();
        // Hold the connection open without sending the rest of the body.
        tokio::time::sleep(Duration::from_secs(120)).await;
        drop(sock);
    });
    let loader = single_url_loader(
        &format!("http://{addr}"),
        Some(crate::client::RetryPolicy {
            max_attempts: 1,
            response_timeout: Some(Duration::from_millis(200)),
            ..crate::client::RetryPolicy::default()
        }),
    );
    let result = tokio::time::timeout(Duration::from_secs(10), loader.load("my_api", "v1"))
        .await
        .expect("the policy timeout must bound the body read");
    server.abort();
    let err = result.unwrap_err();
    assert!(err.to_string().contains("no data received"), "{err}");
}
