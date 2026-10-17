// Copyright 2026 The gws-rust Authors
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

//! Raw payloads streamed to stdout (`-o -`) by generated methods: the bytes
//! arrive complete and in order, and a failing stdout is an error. The API is
//! a local wiremock server; Discovery comes from the repository fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests start gwsr with an environment they set themselves.
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::{Command, Stdio};

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A body large enough to be streamed in many chunks, where every line is
/// different, so a reordered or dropped chunk shows.
fn numbered_lines(count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|i| format!("{i:09}\n").into_bytes())
        .collect()
}

fn gwsr(dir: &Path, base: &str) -> Command {
    let cache = dir.join("cache").join("discovery");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discovery/drive_v3.json"),
        cache.join("drive+v3.json"),
    )
    .unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("gwsr"));
    cmd.env_clear()
        .env("GWSR_CONFIG_DIR", dir)
        .env("GWSR_CACHE_DIR", dir.join("cache"))
        .env("HOME", dir)
        .env("GWSR_TOKEN", "test-token")
        .env("GWSR_API_BASE_URL", base)
        .current_dir(dir);
    for key in ["SystemRoot", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.args([
        "drive",
        "files",
        "get",
        "--params",
        r#"{"fileId":"F","alt":"media"}"#,
        "-o",
        "-",
    ]);
    cmd
}

async fn media_server(body: Vec<u8>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files/F"))
        .and(query_param("alt", "media"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(body),
        )
        .mount(&server)
        .await;
    server
}

/// `-o -` writes the payload exactly: every chunk, in the order received.
/// Writing each chunk through a fresh `tokio::io::stdout()` handle let the
/// chunks race each other on the blocking pool and land out of order.
#[tokio::test(flavor = "multi_thread")]
async fn media_to_stdout_keeps_every_byte_in_order() {
    let body = numbered_lines(800_000);
    let server = media_server(body.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for run in 0..3 {
        let out = gwsr(dir.path(), &server.uri()).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout.len(), body.len(), "run {run}: length");
        let first_difference = out.stdout.iter().zip(&body).position(|(a, b)| a != b);
        assert_eq!(first_difference, None, "run {run}: bytes out of order");
    }
}

/// A stdout that fails with something other than a closed pipe (here: the
/// file size limit, as a full disk would) is reported, not exit 0 with a
/// truncated payload.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn media_to_stdout_reports_a_failing_stdout() {
    let body = numbered_lines(100_000);
    let server = media_server(body).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.bin");
    let gwsr = gwsr(dir.path(), &server.uri());
    // Run gwsr through `sh` to cap the size of the file it writes to at
    // one block (`ulimit -f 1`), with SIGXFSZ ignored so a
    // write past the cap fails with EFBIG instead of killing the process.
    let mut script = String::from("trap '' XFSZ; ulimit -f 1; exec");
    script.push_str(&format!(" '{}'", gwsr.get_program().to_str().unwrap()));
    for arg in gwsr.get_args() {
        script.push_str(&format!(" '{}'", arg.to_str().unwrap()));
    }
    script.push_str(&format!(" > '{}'", target.display()));
    let mut sh = Command::new("sh");
    sh.arg("-c").arg(script).stderr(Stdio::piped());
    sh.env_clear();
    for (key, value) in gwsr.get_envs() {
        if let Some(value) = value {
            sh.env(key, value);
        }
    }
    sh.current_dir(dir.path());
    let out = sh.output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(5), "{stderr}");
    assert!(stderr.contains("failed to write to stdout"), "{stderr}");
}
