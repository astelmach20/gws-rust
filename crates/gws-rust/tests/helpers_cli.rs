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

//! End-to-end tests for the app helpers, run against the real binary.
//!
//! Discovery documents are pre-seeded into an isolated config directory's
//! cache so no network access or credentials are needed; every command either
//! runs with `--dry-run` or is expected to fail before any request is sent.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use serde_json::{Value, json};
use std::path::Path;

fn seed(dir: &Path, name: &str, version: &str, service_path: &str) {
    let cache = dir.join("cache").join("discovery");
    std::fs::create_dir_all(&cache).unwrap();
    let root = if service_path.is_empty() {
        format!("https://{name}.googleapis.com/")
    } else {
        "https://www.googleapis.com/".to_string()
    };
    let doc = json!({
        "name": name,
        "version": version,
        "rootUrl": root,
        "servicePath": service_path,
        "resources": {},
    });
    std::fs::write(
        cache.join(format!("{name}+{version}.json")),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
}

fn gwsr(config: &Path) -> Command {
    let mut cmd = Command::cargo_bin("gwsr").unwrap();
    cmd.env_clear()
        .env("GWSR_CONFIG_DIR", config)
        // The seeded Discovery cache; these tests must never reach the network.
        .env("GWSR_CACHE_DIR", config.join("cache"))
        .env("HOME", config)
        .current_dir(config);
    cmd
}

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path(), "drive", "v3", "drive/v3/");
    seed(dir.path(), "calendar", "v3", "calendar/v3/");
    seed(dir.path(), "docs", "v1", "");
    seed(dir.path(), "sheets", "v4", "");
    seed(dir.path(), "tasks", "v1", "");
    seed(dir.path(), "script", "v1", "");
    dir
}

fn stdout_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn drive_export_dry_run_prints_json_plan() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args([
            "drive",
            "+export",
            "--file-id",
            "DOC-1_a",
            "--to",
            "pdf",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["dry_run"], true);
    assert_eq!(
        v["requests"][0]["url"],
        "https://www.googleapis.com/drive/v3/files/DOC-1_a/export"
    );
    assert_eq!(
        v["requests"][0]["query_params"]["mimeType"],
        "application/pdf"
    );
}

#[test]
fn docs_markdown_write_dry_run_builds_native_requests() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args([
            "docs",
            "+write",
            "--document-id",
            "D",
            "--markdown",
            "--text",
            "# Hi\n\n- a\n- b",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    let body = &v["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r.get("url").is_some())
        .unwrap()["body"];
    let reqs = body["requests"].as_array().unwrap();
    assert_eq!(reqs[0]["insertText"]["text"], "Hi\na\nb");
    assert!(
        reqs.iter()
            .any(|r| r.get("createParagraphBullets").is_some())
    );
}

#[test]
fn sheets_append_dry_run_is_json() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args([
            "sheets",
            "+append",
            "--spreadsheet-id",
            "S",
            "--values",
            "a,\"b,c\"",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["requests"][0]["body"]["values"], json!([["a", "b,c"]]));
}

#[test]
fn destructive_helper_without_yes_fails_before_any_request() {
    // No credentials exist in the isolated config dir, so reaching the auth
    // step would produce an auth error (exit 2). The confirmation gate must
    // refuse first with "confirmation required" (exit 7) naming --yes, and
    // report it as one JSON error on stderr with stdout empty.
    let dir = setup();
    let out = gwsr(dir.path())
        .args(["calendar", "+delete", "--event-id", "E1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stdout.is_empty());
    let err: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["error"]["reason"], "confirmationRequired");
    assert!(
        err["error"]["message"].as_str().unwrap().contains("--yes"),
        "{err}"
    );
}

#[test]
fn outbound_helper_is_gated_only_under_policy() {
    let dir = setup();
    let out = gwsr(dir.path())
        .env("GWSR_REQUIRE_CONFIRM", "1")
        .args([
            "calendar",
            "+insert",
            "--summary",
            "S",
            "--start",
            "2026-01-01T10:00:00Z",
            "--duration",
            "30m",
            "--attendee",
            "a@example.com",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));

    let out = gwsr(dir.path())
        .env("GWSR_REQUIRE_CONFIRM", "maybe")
        .args([
            "calendar",
            "+delete",
            "--event-id",
            "E1",
            "--yes",
            "--dry-run",
        ])
        .output()
        .unwrap();
    // --dry-run proceeds, but a malformed policy value still fails loudly,
    // as a configuration error.
    assert_eq!(out.status.code(), Some(8));
    let err: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["error"]["reason"], "configError", "{err}");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("GWSR_REQUIRE_CONFIRM"),
        "{err}"
    );
}

#[test]
fn dry_run_bypasses_gate_and_prints_json() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args(["calendar", "+delete", "--event-id", "E1", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["requests"][0]["method"], "DELETE");
}

#[test]
fn tasks_due_with_time_is_rejected() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args([
            "tasks",
            "+add",
            "--title",
            "T",
            "--due",
            "2026-04-15T17:00:00Z",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn script_run_keeps_script_id_unreserved_chars() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args([
            "script",
            "+run",
            "--script-id",
            "1-ab_CD",
            "--function",
            "main",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(
        v["requests"][0]["url"],
        "https://script.googleapis.com/v1/scripts/1-ab_CD:run"
    );
}

#[test]
fn upload_rejects_paths_outside_cwd() {
    let dir = setup();
    let out = gwsr(dir.path())
        .args(["drive", "+upload", "--file", "../outside.txt", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
}
