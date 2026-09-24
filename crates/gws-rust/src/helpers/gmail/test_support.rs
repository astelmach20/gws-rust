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

//! Shared test helpers for the Gmail helper modules.

use super::api::GmailApi;
use crate::transport::Transport;
use base64::Engine as _;

/// Extract a header value from raw RFC 5322 output, handling folded lines.
/// Only searches the header block (before the first blank line).
pub(crate) fn extract_header(raw: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name);
    let mut result: Option<String> = None;
    let mut collecting = false;
    for line in raw.lines() {
        if line.is_empty() || line == "\r" {
            break;
        }
        if line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            result = Some(line[prefix.len()..].trim().to_string());
            collecting = true;
        } else if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
            if let Some(ref mut r) = result {
                r.push(' ');
                r.push_str(line.trim());
            }
        } else {
            collecting = false;
        }
    }
    result
}

/// Strip quoted-printable soft line breaks from raw output.
pub(crate) fn strip_qp_soft_breaks(raw: &str) -> String {
    raw.replace("=\r\n", "").replace("=\n", "")
}

/// Base64url-encode a string (as the Gmail API does for body data).
pub(crate) fn base64url(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE.encode(s)
}

/// A `GmailApi` pointed at a wiremock server (paths `/gmail/v1` and `/upload/gmail/v1`).
pub(crate) fn mock_api(server: &wiremock::MockServer) -> GmailApi {
    GmailApi::with_bases(
        Transport::for_test(&server.uri()),
        &format!("{}/gmail/v1", server.uri()),
        &format!("{}/upload/gmail/v1", server.uri()),
    )
}

/// Parse `args` (after `gwsr gmail`) with the real Gmail helper command tree,
/// including the root's global flags, and return the helper's sub-matches.
pub(crate) fn helper_matches(args: &[&str]) -> clap::ArgMatches {
    let doc = crate::discovery::RestDescription {
        name: "gmail".to_string(),
        ..Default::default()
    };
    let mut argv = vec!["gwsr"];
    argv.extend_from_slice(args);
    let m = crate::commands::build_cli(&doc)
        .try_get_matches_from(argv)
        .unwrap_or_else(|e| panic!("failed to parse {args:?}: {e}"));
    match m.subcommand() {
        Some((_, sub)) => sub.clone(),
        None => panic!("no subcommand in {args:?}"),
    }
}
