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

//! The shell completion callback (`GWSR_COMPLETE=<shell>`) treats a failed
//! write to stdout like every other command: a closed pipe is a quiet exit 0
//! and any other failure is an internal error (exit 5), never bad input
//! (exit 3). No test touches the network.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests start gwsr with an environment they set themselves.
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::{Command, Stdio};

fn gwsr(dir: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("gwsr"));
    cmd.env_clear()
        .env("GWSR_CONFIG_DIR", dir.join("config"))
        .env("GWSR_CACHE_DIR", dir.join("cache"))
        .env("HOME", dir)
        .current_dir(dir);
    if let Some(value) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", value);
    }
    cmd
}

/// The registration script and a candidate list, as the shell asks for them.
fn callbacks(dir: &Path) -> Vec<Command> {
    let mut script = gwsr(dir);
    script.env("GWSR_COMPLETE", "zsh");
    let mut candidates = gwsr(dir);
    candidates
        .env("GWSR_COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "1")
        .env("_CLAP_COMPLETE_COMP_TYPE", "9")
        .env("_CLAP_COMPLETE_SPACE", "true")
        .args(["--", "gwsr", ""]);
    vec![script, candidates]
}

#[test]
fn completion_into_a_closed_pipe_exits_quietly() {
    let dir = tempfile::tempdir().unwrap();
    for mut cmd in callbacks(dir.path()) {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = cmd.stdout(writer).stderr(Stdio::piped()).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{:?}: {stderr}", out.status);
        assert!(stderr.is_empty(), "{stderr}");
    }
}

/// A stdout that fails otherwise (here: the file size limit, as a full disk
/// would) is reported as a stdout write failure, exit 5.
#[cfg(unix)]
#[test]
fn completion_into_a_failing_stdout_is_an_internal_error() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.txt");
    // The registration script is longer than the limit.
    for cmd in callbacks(dir.path()).into_iter().take(1) {
        // `sh` caps the size of the file gwsr writes at one block, with
        // SIGXFSZ ignored so a longer write fails with EFBIG instead of
        // killing the process.
        let mut script = format!(
            "trap '' XFSZ; ulimit -f 1; exec '{}'",
            cmd.get_program().to_str().unwrap()
        );
        for arg in cmd.get_args() {
            script.push_str(&format!(" '{}'", arg.to_str().unwrap()));
        }
        script.push_str(&format!(" > '{}'", target.display()));
        let mut sh = Command::new("sh");
        sh.env_clear().arg("-c").arg(script).current_dir(dir.path());
        for (key, value) in cmd.get_envs() {
            if let Some(value) = value {
                sh.env(key, value);
            }
        }
        let out = sh.stderr(Stdio::piped()).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(5), "{stderr}");
        assert!(stderr.contains("failed to write to stdout"), "{stderr}");
    }
}
