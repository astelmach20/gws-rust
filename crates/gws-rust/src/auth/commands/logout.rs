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

//! `gwsr auth logout`: revoke the refresh token at Google, then delete the
//! files gwsr created for the active profile. Files gwsr did not create
//! (`GWSR_CREDENTIALS_FILE`, ADC files, the OAuth client, the encryption key)
//! are never touched.

use std::path::{Path, PathBuf};

use secrecy::ExposeSecret;
use serde_json::json;

use super::{auth_err, print_json};
use crate::auth::credentials::{AuthEnv, Credential, CredentialSource, Resolved};
use crate::auth::http::Endpoints;
use crate::error::GwsError;

pub(super) async fn handle(no_revoke: bool) -> Result<(), GwsError> {
    let env = AuthEnv::from_process().map_err(|e| auth_err(format!("{e:#}")))?;
    let outcome = logout(&env, no_revoke).await?;
    crate::timezone::invalidate_cache()?;
    match outcome.revoke_error {
        // A failure leaves stdout empty; the error says what was removed.
        Some(e) => Err(revoke_failed(&e, &outcome.report)),
        None => print_json(&outcome.report),
    }
}

fn revoke_failed(error: &str, report: &serde_json::Value) -> GwsError {
    let removed = report["removed"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|list| !list.is_empty())
        .unwrap_or_else(|| "none".to_string());
    GwsError::Auth(format!(
        "revoking the refresh token at Google failed: {error}. Local credentials were removed \
         (files: {removed}). Revoke gwsr's access manually at \
         https://myaccount.google.com/permissions"
    ))
}

pub(super) struct Outcome {
    pub report: serde_json::Value,
    pub revoke_error: Option<String>,
}

/// Revocation result.
#[derive(Debug, PartialEq, Eq)]
enum Revocation {
    Revoked,
    AlreadyInvalid,
}

async fn revoke(token: &str, endpoints: &Endpoints) -> Result<Revocation, String> {
    let client = crate::auth::http::client().map_err(|e| format!("{e:#}"))?;
    let resp = client
        .post(&endpoints.revoke_url)
        .form(&[("token", token)])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(Revocation::Revoked);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("HTTP {status}: {e}"))?;
    if status.as_u16() == 400 && body.contains("invalid_token") {
        return Ok(Revocation::AlreadyInvalid);
    }
    Err(format!(
        "HTTP {status}: {}",
        crate::output::sanitize_for_terminal(body.trim())
    ))
}

/// Files in the profile directory that gwsr creates.
fn owned_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ours = matches!(
            name.as_str(),
            "credentials.enc" | "token_cache.enc" | "token_cache.lock" | "profile.json"
        ) || name.starts_with("token_cache.enc.corrupt-");
        if ours {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub(super) async fn logout(env: &AuthEnv, no_revoke: bool) -> Result<Outcome, GwsError> {
    let mut report = json!({
        "status": "success",
        "profile": env.profile.name,
    });
    let mut revoke_error = None;

    let has_profile_creds = env.paths.credentials.exists();
    if has_profile_creds && !no_revoke {
        // Load the profile's own credentials (not whatever GWSR_TOKEN etc. say).
        let mut profile_env = env.clone();
        profile_env.token = None;
        profile_env.token_file = None;
        profile_env.credentials_file = None;
        profile_env.impersonate = None;
        let loaded = tokio::task::spawn_blocking(move || profile_env.load())
            .await
            .map_err(|e| auth_err(format!("credential loading task failed: {e}")))?;
        match loaded {
            Ok((
                CredentialSource::Profile { .. },
                Resolved::Credential(Credential::AuthorizedUser(u)),
            )) => match revoke(u.refresh_token.expose_secret(), &env.endpoints).await {
                Ok(Revocation::Revoked) => report["revoked"] = json!(true),
                Ok(Revocation::AlreadyInvalid) => {
                    report["revoked"] = json!(true);
                    report["revoke_note"] = json!("the token was already expired or revoked");
                }
                Err(e) => {
                    report["revoked"] = json!(false);
                    revoke_error = Some(e);
                }
            },
            Ok(_) => report["revoked"] = json!(false),
            Err(e) => {
                report["revoked"] = json!(false);
                revoke_error = Some(format!("cannot read the stored credentials: {e}"));
            }
        }
    } else if no_revoke {
        report["revoked"] = json!(false);
        report["revoke_note"] = json!("skipped (--no-revoke)");
    }

    let files = owned_files(&env.paths.dir)
        .map_err(|e| auth_err(format!("cannot list '{}': {e}", env.paths.dir.display())))?;
    let mut removed = Vec::new();
    for path in &files {
        std::fs::remove_file(path)
            .map_err(|e| auth_err(format!("cannot remove '{}': {e}", path.display())))?;
        removed.push(path.display().to_string());
    }
    match std::fs::remove_dir(&env.paths.dir) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(e) => {
            return Err(auth_err(format!(
                "cannot remove '{}': {e}",
                env.paths.dir.display()
            )));
        }
    }
    report["removed"] = json!(removed);
    report["message"] = json!(if removed.is_empty() {
        format!("Profile '{}' had no stored credentials.", env.profile.name)
    } else {
        format!("Logged out of profile '{}'.", env.profile.name)
    });

    let mut kept = Vec::new();
    if let Some(p) = &env.credentials_file {
        kept.push(json!({
            "path": p.display().to_string(),
            "reason": "GWSR_CREDENTIALS_FILE is not managed by gwsr; it was left in place",
        }));
    }
    if let Some(p) = &env.token_file {
        kept.push(json!({
            "path": p.display().to_string(),
            "reason": "GWSR_TOKEN_FILE is not managed by gwsr; it was left in place",
        }));
    }
    if !kept.is_empty() {
        report["left_in_place"] = json!(kept);
    }
    if revoke_error.is_some() {
        report["status"] = json!("partial");
    }
    Ok(Outcome {
        report,
        revoke_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::keystore::Purpose;
    use crate::auth::keystore::testing::memory_keystore;
    use crate::auth::profiles::{ActiveProfile, ProfilePaths, ProfileSource};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn logged_in_env(dir: &Path, server: &MockServer) -> AuthEnv {
        let (ks, _) = memory_keystore(dir);
        let paths = ProfilePaths::new(dir, "default");
        paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"c","client_secret":"s","refresh_token":"1//rt"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&paths.credentials, &ct).unwrap();
        crate::fs_util::atomic_write(&paths.metadata, b"{}").unwrap();
        crate::fs_util::atomic_write(&paths.token_cache, b"x").unwrap();
        let mut env = AuthEnv::new(
            dir.to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            paths,
        )
        .with_keystore(ks);
        env.endpoints = crate::auth::token::tests::endpoints(server);
        env
    }

    #[tokio::test]
    async fn revokes_and_removes_only_owned_files() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .and(body_string_contains("token=1%2F%2Frt"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let mut env = logged_in_env(dir.path(), &server);
        let stranger = env.paths.dir.join("notes.txt");
        std::fs::write(&stranger, "mine").unwrap();
        // SEC-11: a user-supplied credentials file outside the profile survives.
        let outside = dir.path().join("sa-key.json");
        std::fs::write(&outside, "{}").unwrap();
        env.credentials_file = Some(outside.clone());

        let out = logout(&env, false).await.unwrap();
        assert!(out.revoke_error.is_none());
        assert_eq!(out.report["revoked"], true);
        assert_eq!(out.report["removed"].as_array().unwrap().len(), 3);
        assert!(!env.paths.credentials.exists());
        assert!(stranger.exists(), "unknown files are not deleted");
        assert!(outside.exists(), "GWSR_CREDENTIALS_FILE is never deleted");
        assert!(
            out.report["left_in_place"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("GWSR_CREDENTIALS_FILE")
        );
    }

    #[tokio::test]
    async fn revoke_failure_still_removes_files_but_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;
        let env = logged_in_env(dir.path(), &server);
        let out = logout(&env, false).await.unwrap();
        let revoke_error = out.revoke_error.clone().unwrap();
        assert!(revoke_error.contains("503"));
        assert_eq!(out.report["status"], "partial");
        let err = revoke_failed(&revoke_error, &out.report);
        let GwsError::Auth(msg) = err else {
            panic!("expected an auth error, got {err:?}")
        };
        assert!(msg.contains("503"), "{msg}");
        let credentials = env.paths.credentials.display().to_string();
        assert!(msg.contains(&credentials), "{msg}");
        assert!(!env.paths.credentials.exists());
        assert!(!env.paths.dir.exists(), "empty profile dir is removed");
    }

    #[tokio::test]
    async fn already_revoked_is_fine_and_no_revoke_skips_network() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .respond_with(
                ResponseTemplate::new(400).set_body_string(r#"{"error":"invalid_token"}"#),
            )
            .mount(&server)
            .await;
        let env = logged_in_env(dir.path(), &server);
        let out = logout(&env, false).await.unwrap();
        assert!(out.revoke_error.is_none());

        let env = logged_in_env(dir.path(), &server);
        let out = logout(&env, true).await.unwrap();
        assert_eq!(out.report["revoked"], false);
        assert!(out.revoke_error.is_none());
    }

    #[tokio::test]
    async fn nothing_to_remove() {
        let dir = tempfile::tempdir().unwrap();
        let env = AuthEnv::new(
            dir.path().to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(dir.path(), "default"),
        );
        let out = logout(&env, false).await.unwrap();
        assert!(out.report["removed"].as_array().unwrap().is_empty());
    }
}
