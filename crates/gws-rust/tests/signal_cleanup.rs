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

//! A SIGINT or SIGTERM while `gwsr` writes an output file: the partial
//! temporary file is removed and the process exits like one stopped by the
//! signal (130 or 143). The API is a local server that sends part of a body
//! and then stalls; Discovery comes from the repository fixtures.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests start gwsr and `kill` with environments they set themselves.
#![allow(clippy::disallowed_methods)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Serve every request with the headers and the first 64 KiB of a 10 MiB
/// body, then keep the connection open without sending more.
fn stalling_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { break };
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match conn.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let head = "HTTP/1.1 200 OK\r\n\
                        content-type: application/octet-stream\r\n\
                        content-length: 10485760\r\n\r\n";
            if conn.write_all(head.as_bytes()).is_ok() && conn.write_all(&[b'x'; 65536]).is_ok() {
                held.push(conn);
            }
        }
    });
    format!("http://{addr}")
}

fn temp_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".gwsr-output-"))
        .collect()
}

#[test]
fn a_signal_during_a_download_removes_the_temporary_file() {
    let base = stalling_server();
    for (signal, code) in [("TERM", 143), ("INT", 130)] {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache").join("discovery");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discovery/drive_v3.json"),
            cache.join("drive+v3.json"),
        )
        .unwrap();
        let out_dir = dir.path().join("out");
        std::fs::create_dir(&out_dir).unwrap();
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("gwsr"));
        child
            .env_clear()
            .env("GWSR_CONFIG_DIR", dir.path())
            .env("GWSR_CACHE_DIR", dir.path().join("cache"))
            .env("HOME", dir.path())
            .env("GWSR_TOKEN", "test-token")
            .env("GWSR_API_BASE_URL", &base)
            .current_dir(dir.path())
            .args([
                "drive",
                "files",
                "get",
                "--params",
                r#"{"fileId":"F","alt":"media"}"#,
                "-o",
            ])
            .arg(out_dir.join("file.bin"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = child.spawn().unwrap();

        // Wait until the download has started writing its temporary file.
        let deadline = Instant::now() + Duration::from_secs(10);
        while temp_files(&out_dir).is_empty() {
            assert!(Instant::now() < deadline, "the download never started");
            std::thread::sleep(Duration::from_millis(20));
        }
        let kill = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(child.id().to_string())
            .status()
            .unwrap();
        assert!(kill.success());
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(code), "SIG{signal}: {stderr}");
        assert!(out.stdout.is_empty(), "SIG{signal}: no summary on stdout");
        assert_eq!(
            temp_files(&out_dir),
            Vec::<String>::new(),
            "SIG{signal} left the partial download behind"
        );
        assert!(!out_dir.join("file.bin").exists(), "SIG{signal}");
    }
}
