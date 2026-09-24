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

//! `gwsr auth list` and `gwsr auth use <profile>`.

use std::path::Path;

use serde_json::{Value, json};

use super::{auth_err, print_json};
use crate::auth::profiles::{self, ActiveProfile, ProfilePaths};
use crate::error::GwsError;

pub(super) fn handle_list() -> Result<(), GwsError> {
    let base = profiles::try_config_dir().map_err(|e| auth_err(format!("{e:#}")))?;
    let active = profiles::active_profile(&base).map_err(|e| auth_err(format!("{e:#}")))?;
    print_json(&list_report(&base, &active)?)
}

pub(super) fn list_report(base: &Path, active: &ActiveProfile) -> Result<Value, GwsError> {
    let names = profiles::list_profiles(base).map_err(|e| auth_err(format!("{e:#}")))?;
    let mut rows = Vec::new();
    for name in names {
        let paths = ProfilePaths::new(base, &name);
        let meta =
            profiles::load_metadata(&paths.metadata).map_err(|e| auth_err(format!("{e:#}")))?;
        rows.push(json!({
            "name": name,
            "active": name == active.name,
            "logged_in": paths.credentials.exists(),
            "account": meta.as_ref().and_then(|m| m.account.clone()),
            "granted_scope_count": meta.as_ref().map(|m| m.granted_scopes.len()),
        }));
    }
    Ok(json!({
        "active_profile": active.name,
        "active_profile_source": active.source,
        "profiles": rows,
    }))
}

pub(super) fn handle_use(m: &clap::ArgMatches) -> Result<(), GwsError> {
    let name = m
        .get_one::<String>("name")
        .ok_or_else(|| GwsError::Validation("a profile name is required".into()))?;
    let base = profiles::try_config_dir().map_err(|e| auth_err(format!("{e:#}")))?;
    let mut report = use_profile(&base, name)?;
    if std::env::var("GWSR_PROFILE").is_ok_and(|v| !v.is_empty()) {
        report["warning"] = json!(
            "GWSR_PROFILE is set in this environment and takes precedence over `gwsr auth use`"
        );
    }
    print_json(&report)
}

pub(super) fn use_profile(base: &Path, name: &str) -> Result<Value, GwsError> {
    profiles::validate_profile_name(name).map_err(|e| GwsError::Validation(format!("{e:#}")))?;
    let paths = ProfilePaths::new(base, name);
    if !paths.credentials.exists() {
        return Err(GwsError::Validation(format!(
            "profile '{name}' has no credentials; create it with `gwsr auth login --profile {name}`"
        )));
    }
    profiles::set_active_profile(base, name).map_err(|e| auth_err(format!("{e:#}")))?;
    Ok(json!({"status": "success", "active_profile": name}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::profiles::{ProfileMetadata, ProfileSource};

    #[test]
    fn use_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        assert!(use_profile(base, "work").is_err(), "unknown profile");
        assert!(use_profile(base, "../x").is_err(), "invalid name");

        let work = ProfilePaths::new(base, "work");
        work.ensure_dir().unwrap();
        std::fs::write(&work.credentials, b"GWSR").unwrap();
        profiles::save_metadata(
            &work.metadata,
            &ProfileMetadata {
                account: Some("w@example.com".into()),
                client_id: "c".into(),
                granted_scopes: vec!["a".into(), "b".into()],
                updated_at: "t".into(),
            },
        )
        .unwrap();
        ProfilePaths::new(base, "empty").ensure_dir().unwrap();

        use_profile(base, "work").unwrap();
        let active = profiles::active_profile(base).unwrap();
        assert_eq!(active.name, "work");
        assert_eq!(active.source, ProfileSource::ActiveProfileFile);

        let r = list_report(base, &active).unwrap();
        assert_eq!(r["active_profile"], "work");
        let rows = r["profiles"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "empty");
        assert_eq!(rows[0]["logged_in"], false);
        assert_eq!(rows[1]["account"], "w@example.com");
        assert_eq!(rows[1]["active"], true);
        assert_eq!(rows[1]["granted_scope_count"], 2);
    }
}
