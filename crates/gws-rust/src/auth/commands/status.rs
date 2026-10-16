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

//! `gwsr auth status`: an honest report of what would be used and whether it
//! works. Problems are reported as fields; nothing is modified.

use serde_json::{Value, json};

use super::print_json;
use crate::auth::client_config;
use crate::auth::credentials::{AuthEnv, Credential, CredentialSource, Resolved};
use crate::auth::profiles;
use crate::error::GwsError;

pub(super) async fn handle(offline: bool) -> Result<(), GwsError> {
    let env = AuthEnv::from_process().map_err(crate::auth::to_gws_error)?;
    let report = build_report(&env, offline).await;
    print_json(&report)
}

pub(super) async fn build_report(env: &AuthEnv, offline: bool) -> Value {
    let mut out = json!({
        "config_dir": env.base.display().to_string(),
        "profile": env.profile.name,
        "profile_source": env.profile.source,
    });

    match profiles::list_profiles(&env.base) {
        Ok(dirs) => {
            out["profiles"] = json!(dirs.valid);
            let (invalid, warning) = super::profile::invalid_profiles_report(&dirs);
            out["invalid_profiles"] = invalid;
            if let Some(w) = warning {
                out["invalid_profiles_warning"] = json!(w);
            }
        }
        Err(e) => out["profiles_error"] = json!(format!("{e:#}")),
    }

    out["keyring_backend"] = json!(env.keyring_backend.as_str());

    // Report the client `auth login` would use (same resolution order), and
    // name a saved file that the environment variables shadow.
    match client_config::resolve_optional_in(&env.base) {
        Ok(Some(c)) => {
            let mut client = json!({
                "source": c.source.kind(),
                "client_id": client_config::mask_client_id(&c.client_id),
                "project_id": c.project_id,
            });
            if c.source == client_config::ClientSource::EnvVars {
                let p = client_config::client_config_path_in(&env.base);
                if p.exists() {
                    client["overrides"] = json!(p.display().to_string());
                }
            }
            out["client"] = client;
        }
        Ok(None) => out["client"] = Value::Null,
        Err(e) => out["client_error"] = json!(format!("{e:#}")),
    }

    if let Some(sub) = &env.impersonate {
        out["impersonate"] = json!(sub);
    }

    let source = match env.source() {
        Ok(s) => s,
        Err(e) => {
            out["authenticated"] = json!(false);
            out["credential_error"] = json!(e.to_string());
            return out;
        }
    };
    out["credential_source"] = json!(source.kind());
    if let Some(p) = source.path() {
        out["credential_path"] = json!(p.display().to_string());
    }
    if let CredentialSource::CredentialsFile(_) = source
        && env.paths.credentials.exists()
    {
        out["note"] = json!(format!(
            "GWSR_CREDENTIALS_FILE overrides the stored credentials of profile '{}'",
            env.profile.name
        ));
    }

    if let CredentialSource::Profile { .. } = source {
        out["key_location"] = json!(env.keystore().describe());
        match profiles::load_metadata(&env.paths.metadata) {
            Ok(Some(m)) => {
                out["account"] = json!(m.account);
                out["granted_scopes"] = json!(m.granted_scopes);
                out["logged_in_at"] = json!(m.updated_at);
            }
            Ok(None) => out["metadata"] = json!("missing (granted scopes unknown)"),
            Err(e) => out["metadata_error"] = json!(format!("{e:#}")),
        }
    }

    let load_env = env.clone();
    let loaded = match tokio::task::spawn_blocking(move || load_env.load()).await {
        Ok(r) => r,
        Err(e) => {
            out["credential_error"] = json!(format!("credential loading task failed: {e}"));
            return out;
        }
    };
    let credential = match loaded {
        Ok((_, Resolved::Token(_))) => {
            out["credential_type"] = json!("access_token");
            out["authenticated"] = json!(true);
            return out;
        }
        Ok((_, Resolved::Credential(c))) => c,
        Err(e) => {
            out["authenticated"] = json!(false);
            out["credential_error"] = json!(e.to_string());
            return out;
        }
    };
    out["credential_type"] = json!(credential.kind());
    out["identity"] = json!(match &credential {
        Credential::AuthorizedUser(u) => client_config::mask_client_id(&u.client_id),
        Credential::ServiceAccount(sa) => sa.client_email.clone(),
    });

    if offline {
        out["authenticated"] = json!(true);
        out["verified"] = json!(false);
        return out;
    }
    match &credential {
        Credential::AuthorizedUser(user) => {
            let login = crate::auth::login_command(env);
            let remedy = crate::auth::rejected_grant_remedy(env, &source);
            match crate::auth::token::refresh_user(user, None, &env.endpoints, &login)
                .await
                .map_err(|e| e.with_grant_remedy(&remedy))
            {
                Ok(t) => {
                    out["authenticated"] = json!(true);
                    out["verified"] = json!(true);
                    if let Some(scopes) = t.scopes {
                        out["token_scopes"] = json!(scopes);
                    }
                }
                // No response from Google, or a server-side failure (408,
                // 429, 5xx): nothing was verified, so report it like
                // `--offline` instead of as a rejected credential.
                Err(
                    e @ (crate::auth::AuthError::Network(_)
                    | crate::auth::AuthError::TokenEndpointUnavailable { .. }
                    | crate::auth::AuthError::TokenEndpointFailed { .. }),
                ) => {
                    out["authenticated"] = json!(true);
                    out["verified"] = json!(false);
                    out["verification_error"] = json!(e.to_string());
                }
                Err(e) => {
                    out["authenticated"] = json!(false);
                    out["verified"] = json!(true);
                    out["token_error"] = json!(e.to_string());
                }
            }
        }
        Credential::ServiceAccount(_) => {
            out["authenticated"] = json!(true);
            out["verified"] = json!(false);
            out["verification_note"] =
                json!("service-account keys are verified on first use with the scopes of the call");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::keystore::Purpose;
    use crate::auth::keystore::testing::memory_keystore;
    use crate::auth::profiles::{ActiveProfile, ProfileMetadata, ProfilePaths, ProfileSource};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn env(dir: &std::path::Path) -> AuthEnv {
        AuthEnv::new(
            dir.to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(dir, "default"),
        )
    }

    /// The report reads the OAuth client from the config directory it is
    /// given, never from the process-wide (real) one.
    #[tokio::test]
    async fn client_is_read_from_the_reports_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        crate::auth::client_config::save_client_config(
            &dir.path().join("client_secret.json"),
            "123-hermetic.apps.googleusercontent.com",
            &secrecy::SecretString::from("cs".to_string()),
            Some("hermetic-test-project"),
        )
        .unwrap();
        let r = build_report(&env(dir.path()), true).await;
        // No message: on failure the report would describe the real client.
        assert!(r["client"]["project_id"] == "hermetic-test-project");
        assert!(r["client"]["source"] == "client_secret.json");
    }

    #[tokio::test]
    async fn no_credentials_report() {
        let dir = tempfile::tempdir().unwrap();
        let r = build_report(&env(dir.path()), true).await;
        assert_eq!(r["authenticated"], false);
        assert!(
            r["credential_error"]
                .as_str()
                .unwrap()
                .contains("No credentials")
        );
        assert_eq!(r["profile"], "default");
        assert_eq!(r["invalid_profiles"], json!([]));
        assert!(r.get("invalid_profiles_warning").is_none(), "{r}");
    }

    /// A profile directory with a name gwsr no longer accepts is reported,
    /// not silently left out of `profiles`.
    #[tokio::test]
    async fn invalid_profile_directories_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("profiles").join("Work")).unwrap();
        let r = build_report(&env(dir.path()), true).await;
        assert_eq!(r["profiles"], json!([]));
        assert_eq!(r["invalid_profiles"][0]["name"], "Work");
        assert_eq!(r["invalid_profiles"][0]["suggested_name"], "work");
        let w = r["invalid_profiles_warning"].as_str().unwrap();
        assert!(w.contains("'Work' (rename to 'work')"), "{w}");
    }

    #[tokio::test]
    async fn profile_report_verifies_with_google() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.x", "expires_in": 3600, "token_type": "Bearer",
                "scope": "openid https://www.googleapis.com/auth/drive.readonly"
            })))
            .mount(&server)
            .await;
        let (ks, _) = memory_keystore(dir.path());
        let mut e = env(dir.path());
        e.paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&e.paths.credentials, &ct).unwrap();
        crate::auth::profiles::save_metadata(
            &e.paths.metadata,
            &ProfileMetadata {
                account: Some("me@example.com".into()),
                client_id: "cid".into(),
                granted_scopes: vec!["openid".into()],
                updated_at: "t".into(),
            },
        )
        .unwrap();
        e.endpoints = crate::auth::token::tests::endpoints(&server);
        let e = e.with_keystore(ks);

        let r = build_report(&e, false).await;
        assert_eq!(r["authenticated"], true, "{r}");
        assert_eq!(r["verified"], true);
        assert_eq!(r["credential_source"], "profile");
        assert_eq!(r["account"], "me@example.com");
        assert_eq!(r["key_location"], "memory");
        assert_eq!(
            r["token_scopes"][1],
            "https://www.googleapis.com/auth/drive.readonly"
        );
        let text = r.to_string();
        assert!(
            !text.contains("\"rt\"") && !text.contains("\"cs\""),
            "no secrets: {text}"
        );
    }

    /// Without a response from the token endpoint nothing was verified: the
    /// report must not claim a verified "not authenticated".
    #[tokio::test]
    async fn unreachable_token_endpoint_is_not_a_verified_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (ks, _) = memory_keystore(dir.path());
        let mut e = env(dir.path());
        e.paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&e.paths.credentials, &ct).unwrap();
        // A port nothing listens on: the refresh gets no response at all.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        e.endpoints.token_url = format!("http://127.0.0.1:{port}/token");
        let e = e.with_keystore(ks);

        let r = build_report(&e, false).await;
        assert_eq!(r["verified"], false, "{r}");
        assert_ne!(r["authenticated"], false, "{r}");
        assert!(r["verification_error"].is_string(), "{r}");
    }

    /// A rejected refresh token tells the user how to sign in again. For a
    /// profile chosen with `--profile`, plain `gwsr auth login` would sign in
    /// to a different profile, so the hint must name the active one.
    /// `gwsr auth login` cannot fix a rejected GWSR_CREDENTIALS_FILE (it
    /// takes precedence over every profile): the report names the file.
    #[tokio::test]
    async fn invalid_grant_from_credentials_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let file = dir.path().join("user.json");
        crate::fs_util::atomic_write(
            &file,
            br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
        )
        .unwrap();
        let mut e = env(dir.path());
        e.credentials_file = Some(file.clone());
        e.endpoints = crate::auth::token::tests::endpoints(&server);

        let r = build_report(&e, false).await;
        assert_eq!(r["authenticated"], false, "{r}");
        let err = r["token_error"].as_str().unwrap_or_default();
        assert!(err.contains("GWSR_CREDENTIALS_FILE"), "{r}");
        assert!(err.contains(&file.display().to_string()), "{r}");
        assert!(!err.contains("to sign in again"), "{r}");
    }

    #[tokio::test]
    async fn invalid_grant_hint_names_the_active_profile() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let (ks, _) = memory_keystore(dir.path());
        let mut e = AuthEnv::new(
            dir.path().to_path_buf(),
            ActiveProfile {
                name: "work".into(),
                source: ProfileSource::Flag,
            },
            ProfilePaths::new(dir.path(), "work"),
        );
        e.paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&e.paths.credentials, &ct).unwrap();
        e.endpoints = crate::auth::token::tests::endpoints(&server);
        let e = e.with_keystore(ks);

        let r = build_report(&e, false).await;
        assert_eq!(r["authenticated"], false, "{r}");
        let err = r["token_error"].as_str().unwrap_or_default();
        assert!(err.contains("Run `gwsr auth login --profile work`"), "{r}");
    }

    /// A 408, 429 or 5xx from the token endpoint is a failure on Google's side, not a
    /// rejected credential (the error itself says "The credentials were not
    /// rejected"). It must not be reported as a verified "not authenticated".
    #[tokio::test]
    async fn token_endpoint_outage_is_not_a_verified_failure() {
        for status in [408, 429, 500, 501, 503] {
            let dir = tempfile::tempdir().unwrap();
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(ResponseTemplate::new(status).set_body_string("backend error"))
                .mount(&server)
                .await;
            let (ks, _) = memory_keystore(dir.path());
            let mut e = env(dir.path());
            e.paths.ensure_dir().unwrap();
            let ct = ks
                .encrypt(
                    Purpose::Credentials,
                    br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
                )
                .unwrap();
            crate::fs_util::atomic_write(&e.paths.credentials, &ct).unwrap();
            e.endpoints = crate::auth::token::tests::endpoints(&server);
            let e = e.with_keystore(ks);

            let r = build_report(&e, false).await;
            assert_eq!(r["verified"], false, "HTTP {status}: {r}");
            assert_ne!(r["authenticated"], false, "HTTP {status}: {r}");
            assert!(r.get("token_error").is_none(), "HTTP {status}: {r}");
            let detail = r["verification_error"].as_str().unwrap_or_default();
            assert!(detail.contains("credentials were not rejected"), "{r}");
        }
    }

    /// An error status without an OAuth error body (a proxy asking for
    /// authentication, a captive portal's redirect or HTML page) is not
    /// Google's verdict on the credential, so it must not be reported as a
    /// verified "not authenticated" either. A real OAuth error still is.
    #[tokio::test]
    async fn non_oauth_error_response_is_not_a_verified_failure() {
        let cases = [
            (
                407,
                ResponseTemplate::new(407)
                    .set_body_string("<html>Proxy Authentication Required</html>"),
            ),
            (302, ResponseTemplate::new(302)),
            (
                403,
                ResponseTemplate::new(403).set_body_string("<html>Sign in to the Wi-Fi</html>"),
            ),
            (
                401,
                ResponseTemplate::new(401).set_body_json(serde_json::json!({"code": 401})),
            ),
            (
                200,
                ResponseTemplate::new(200).set_body_string("<html>Sign in to the Wi-Fi</html>"),
            ),
        ];
        for (status, template) in cases {
            let dir = tempfile::tempdir().unwrap();
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(template)
                .mount(&server)
                .await;
            let r = build_report(&profile_env(dir.path(), &server), false).await;
            assert_eq!(r["verified"], false, "HTTP {status}: {r}");
            assert_eq!(r["authenticated"], true, "HTTP {status}: {r}");
            assert!(r.get("token_error").is_none(), "HTTP {status}: {r}");
            let detail = r["verification_error"].as_str().unwrap_or_default();
            assert!(detail.contains(&format!("HTTP {status}")), "{r}");
            assert!(detail.contains("credentials were not rejected"), "{r}");
        }

        // Google's OAuth error is a verdict: still a verified failure.
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "invalid_client",
                "error_description": "The OAuth client was not found."
            })))
            .mount(&server)
            .await;
        let r = build_report(&profile_env(dir.path(), &server), false).await;
        assert_eq!(r["verified"], true, "{r}");
        assert_eq!(r["authenticated"], false, "{r}");
        assert!(
            r["token_error"]
                .as_str()
                .unwrap_or_default()
                .contains("invalid_client"),
            "{r}"
        );
    }

    /// A default-profile environment with a stored user credential whose
    /// token endpoint is `server`.
    fn profile_env(dir: &std::path::Path, server: &MockServer) -> AuthEnv {
        let (ks, _) = memory_keystore(dir);
        let mut e = env(dir);
        e.paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"cid-1234567890.apps","client_secret":"cs","refresh_token":"rt"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&e.paths.credentials, &ct).unwrap();
        e.endpoints = crate::auth::token::tests::endpoints(server);
        e.with_keystore(ks)
    }
}
