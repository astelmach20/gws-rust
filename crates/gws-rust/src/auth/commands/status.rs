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

use super::{auth_err, print_json};
use crate::auth::client_config;
use crate::auth::credentials::{AuthEnv, Credential, CredentialSource, Resolved};
use crate::auth::keystore::BackendKind;
use crate::auth::profiles;
use crate::error::GwsError;

pub(super) async fn handle(offline: bool) -> Result<(), GwsError> {
    let env = AuthEnv::from_process().map_err(|e| auth_err(format!("{e:#}")))?;
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
        Ok(list) => out["profiles"] = json!(list),
        Err(e) => out["profiles_error"] = json!(format!("{e:#}")),
    }

    match BackendKind::from_env() {
        Ok(kind) => out["keyring_backend"] = json!(kind.as_str()),
        Err(e) => out["keyring_backend_error"] = json!(e.to_string()),
    }

    match client_config::client_config_path().and_then(|p| client_config::load_from(&p)) {
        Ok(Some(c)) => {
            out["client"] = json!({
                "source": c.source.kind(),
                "client_id": client_config::mask_client_id(&c.client_id),
                "project_id": c.project_id,
            });
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
        match env.keystore() {
            Ok(ks) => out["key_location"] = json!(ks.describe()),
            Err(e) => out["key_error"] = json!(e.to_string()),
        }
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
            match crate::auth::token::refresh_user(user, None, &env.endpoints, "gwsr auth login")
                .await
            {
                Ok(t) => {
                    out["authenticated"] = json!(true);
                    out["verified"] = json!(true);
                    if let Some(scopes) = t.scopes {
                        out["token_scopes"] = json!(scopes);
                    }
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
}
