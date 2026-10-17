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

//! `drive +sync` under `GWSR_RESTRICT_PATHS=cwd`: the local tree it builds
//! from Drive folder names must stay inside the current directory, also
//! through a symlinked sub-directory of `--dir`.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Tests start gwsr with environments they set themselves.
#![allow(clippy::disallowed_methods)]

use assert_cmd::Command;
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FOLDER: &str = "application/vnd.google-apps.folder";

fn seed_drive(config: &Path) {
    let cache = config.join("cache").join("discovery");
    std::fs::create_dir_all(&cache).unwrap();
    let doc = json!({
        "name": "drive",
        "version": "v3",
        "rootUrl": "https://www.googleapis.com/",
        "servicePath": "drive/v3/",
        "resources": {},
    });
    std::fs::write(
        cache.join("drive+v3.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
}

async fn mount_folder(server: &MockServer, id: &str, children: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/drive/v3/files/{id}")))
        .and(query_param("fields", "id,mimeType,trashed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": id, "mimeType": FOLDER})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param(
            "q",
            format!("'{id}' in parents and trashed = false"),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": children})))
        .mount(server)
        .await;
}

/// `Sub` in the mirror is a symlink to a directory outside the working
/// directory. Drive folder `Sub` holds an empty folder `Empty`, so the sync
/// writes no file there, only the directory. That directory must not be
/// created through the link: the run fails, as a file download through it
/// does, and nothing appears outside.
#[tokio::test(flavor = "multi_thread")]
async fn sync_creates_no_directory_outside_cwd_through_a_symlink() {
    let server = MockServer::start().await;
    mount_folder(
        &server,
        "ROOT",
        json!([{"id": "S", "name": "Sub", "mimeType": FOLDER}]),
    )
    .await;
    mount_folder(
        &server,
        "S",
        json!([{"id": "E", "name": "Empty", "mimeType": FOLDER}]),
    )
    .await;
    mount_folder(&server, "E", json!([])).await;

    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let work = tmp.path().join("work");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(work.join("mirror")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, work.join("mirror").join("Sub")).unwrap();
    seed_drive(&config);

    let sync = || {
        let mut cmd = Command::cargo_bin("gwsr").unwrap();
        cmd.env_clear();
        // The Nix build sandbox has no system trust store; building the HTTP
        // client needs its certificate bundle even for a plain-HTTP server.
        for key in ["SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("GWSR_CONFIG_DIR", &config)
            .env("GWSR_CACHE_DIR", config.join("cache"))
            .env("HOME", &config)
            .env("GWSR_TOKEN", "test-token")
            .env("GWSR_API_BASE_URL", server.uri())
            .env("GWSR_RESTRICT_PATHS", "cwd")
            .current_dir(&work)
            .args(["drive", "+sync", "--folder-id", "ROOT", "--dir", "mirror"])
            .output()
            .unwrap()
    };
    let out = sync();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !outside.join("Empty").exists(),
        "+sync created a directory outside the current directory; stderr: {stderr}"
    );
    assert_eq!(out.status.code(), Some(3), "stderr: {stderr}");
    assert!(stderr.contains("GWSR_RESTRICT_PATHS"), "{stderr}");

    // Without the link the same tree is mirrored inside the directory.
    std::fs::remove_file(work.join("mirror").join("Sub")).unwrap();
    let out = sync();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(work.join("mirror").join("Sub").join("Empty").is_dir());
}
