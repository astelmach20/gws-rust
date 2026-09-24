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

//! The Workspace APIs `gwsr auth setup` can enable, and scope metadata
//! discovered from their Discovery documents.

use crate::auth::scopes;

/// A Workspace API: service ID, display name and Discovery coordinates.
pub struct ApiEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub discovery: &'static str,
    pub version: &'static str,
}

macro_rules! api {
    ($id:literal, $name:literal, $disc:literal, $ver:literal) => {
        ApiEntry {
            id: $id,
            name: $name,
            discovery: $disc,
            version: $ver,
        }
    };
}

/// Every API `gwsr auth setup` offers to enable.
pub const WORKSPACE_APIS: &[ApiEntry] = &[
    api!("drive.googleapis.com", "Google Drive", "drive", "v3"),
    api!("sheets.googleapis.com", "Google Sheets", "sheets", "v4"),
    api!("gmail.googleapis.com", "Gmail", "gmail", "v1"),
    api!(
        "calendar-json.googleapis.com",
        "Google Calendar",
        "calendar",
        "v3"
    ),
    api!("docs.googleapis.com", "Google Docs", "docs", "v1"),
    api!("slides.googleapis.com", "Google Slides", "slides", "v1"),
    api!("tasks.googleapis.com", "Google Tasks", "tasks", "v1"),
    api!("people.googleapis.com", "People (Contacts)", "people", "v1"),
    api!("chat.googleapis.com", "Google Chat", "chat", "v1"),
    api!("vault.googleapis.com", "Google Vault", "vault", "v1"),
    api!(
        "groupssettings.googleapis.com",
        "Groups Settings",
        "groupssettings",
        "v1"
    ),
    api!("reseller.googleapis.com", "Reseller", "reseller", "v1"),
    api!("licensing.googleapis.com", "Licensing", "licensing", "v1"),
    api!("script.googleapis.com", "Apps Script", "script", "v1"),
    api!("admin.googleapis.com", "Admin SDK", "admin", "directory_v1"),
    api!("classroom.googleapis.com", "Classroom", "classroom", "v1"),
    api!(
        "cloudidentity.googleapis.com",
        "Cloud Identity",
        "cloudidentity",
        "v1"
    ),
    api!(
        "alertcenter.googleapis.com",
        "Alert Center",
        "alertcenter",
        "v1beta1"
    ),
    api!("forms.googleapis.com", "Google Forms", "forms", "v1"),
    api!("keep.googleapis.com", "Google Keep", "keep", "v1"),
    api!("meet.googleapis.com", "Google Meet", "meet", "v2"),
    api!(
        "driveactivity.googleapis.com",
        "Drive Activity",
        "driveactivity",
        "v2"
    ),
    api!(
        "drivelabels.googleapis.com",
        "Drive Labels",
        "drivelabels",
        "v2"
    ),
    api!(
        "chromemanagement.googleapis.com",
        "Chrome Management",
        "chromemanagement",
        "v1"
    ),
    api!(
        "chromepolicy.googleapis.com",
        "Chrome Policy",
        "chromepolicy",
        "v1"
    ),
    api!(
        "gmailpostmastertools.googleapis.com",
        "Gmail Postmaster Tools",
        "gmailpostmastertools",
        "v2"
    ),
    api!(
        "cloudsearch.googleapis.com",
        "Cloud Search",
        "cloudsearch",
        "v1"
    ),
    api!(
        "workspaceevents.googleapis.com",
        "Workspace Events",
        "workspaceevents",
        "v1"
    ),
    api!("pubsub.googleapis.com", "Cloud Pub/Sub", "pubsub", "v1"),
];

const RESTRICTED_SCOPES: &[&str] = &[
    "chat.admin.delete",
    "chat.delete",
    "chat.messages",
    "chat.messages.readonly",
    "drive",
    "drive.activity",
    "drive.activity.readonly",
    "drive.meet.readonly",
    "drive.metadata",
    "drive.metadata.readonly",
    "drive.readonly",
    "drive.scripts",
    "gmail.compose",
    "gmail.insert",
    "gmail.metadata",
    "gmail.modify",
    "gmail.readonly",
    "gmail.settings.basic",
    "gmail.settings.sharing",
];

const SENSITIVE_SCOPES: &[&str] = &[
    "chat.admin.memberships",
    "chat.admin.memberships.readonly",
    "chat.admin.spaces",
    "chat.admin.spaces.readonly",
    "chat.customemojis",
    "chat.customemojis.readonly",
    "documents",
    "documents.readonly",
    "chat.memberships",
    "chat.memberships.app",
    "chat.memberships.readonly",
    "chat.messages.create",
    "chat.messages.reactions",
    "chat.messages.reactions.create",
    "chat.messages.reactions.readonly",
    "chat.spaces",
    "chat.spaces.create",
    "chat.spaces.readonly",
    "chat.users.readstate",
    "chat.users.readstate.readonly",
    "chat.users.spacesettings",
    "drive.apps.readonly",
    "gmail.addons.current.message.metadata",
    "gmail.addons.current.message.readonly",
    "gmail.send",
];

/// Google's sensitivity class for a scope (affects app verification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeClassification {
    NonSensitive,
    Sensitive,
    Restricted,
}

/// Classify a scope URL.
pub fn classify(url: &str) -> ScopeClassification {
    if url == scopes::GMAIL_FULL_SCOPE {
        return ScopeClassification::Restricted;
    }
    let short = scopes::short_name(url);
    if RESTRICTED_SCOPES.contains(&short) {
        ScopeClassification::Restricted
    } else if SENSITIVE_SCOPES.contains(&short) {
        ScopeClassification::Sensitive
    } else {
        ScopeClassification::NonSensitive
    }
}

/// A scope discovered from a Discovery document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredScope {
    pub url: String,
    pub short: String,
    pub description: String,
    pub is_readonly: bool,
    pub classification: ScopeClassification,
}

fn friendly_name(short: &str) -> String {
    short
        .split('.')
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scopes from one Discovery document, excluding app-only and legacy feeds.
pub fn scopes_from_document(doc: &crate::discovery::RestDescription) -> Vec<DiscoveredScope> {
    let Some(map) = doc
        .auth
        .as_ref()
        .and_then(|a| a.oauth2.as_ref())
        .and_then(|o| o.scopes.as_ref())
    else {
        return Vec::new();
    };
    map.iter()
        .filter(|(url, _)| url.starts_with(scopes::SCOPE_PREFIX) && !scopes::is_app_only_scope(url))
        .map(|(url, desc)| {
            let short = scopes::short_name(url).to_string();
            let description = desc
                .description
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| friendly_name(&short));
            DiscoveredScope {
                is_readonly: short.contains("readonly"),
                classification: classify(url),
                url: url.clone(),
                short,
                description,
            }
        })
        .collect()
}

/// Fetch scopes for the enabled APIs (by service ID). APIs whose Discovery
/// document cannot be fetched are reported on stderr and skipped.
pub async fn fetch_scopes_for_apis(enabled_api_ids: &[String]) -> Vec<DiscoveredScope> {
    let mut all: Vec<DiscoveredScope> = Vec::new();
    for api in WORKSPACE_APIS {
        if !enabled_api_ids.iter().any(|id| id == api.id) {
            continue;
        }
        match crate::discovery::fetch_discovery_document(api.discovery, api.version).await {
            Ok(doc) => {
                for s in scopes_from_document(&doc) {
                    if !all.iter().any(|e| e.url == s.url) {
                        all.push(s);
                    }
                }
            }
            Err(e) => tracing::warn!("cannot load scopes for {} ({}): {e:#}", api.name, api.id),
        }
    }
    all.sort_by(|a, b| {
        b.classification
            .cmp(&a.classification)
            .then_with(|| a.short.cmp(&b.short))
    });
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn api_ids_are_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for api in WORKSPACE_APIS {
            assert!(api.id.ends_with(".googleapis.com"), "{}", api.id);
            assert!(seen.insert(api.id), "duplicate {}", api.id);
        }
    }

    #[test]
    fn api_ids_cover_registered_services() {
        let ids: Vec<&str> = WORKSPACE_APIS.iter().map(|a| a.id).collect();
        for entry in crate::services::SERVICES {
            let expected = match entry.api_name {
                "modelarmor" | "workflow" => continue,
                "calendar" => "calendar-json.googleapis.com".to_string(),
                other => format!("{other}.googleapis.com"),
            };
            assert!(ids.contains(&expected.as_str()), "missing {expected}");
        }
    }

    #[test]
    fn classification() {
        assert_eq!(
            classify("https://mail.google.com/"),
            ScopeClassification::Restricted
        );
        assert_eq!(
            classify("https://www.googleapis.com/auth/gmail.send"),
            ScopeClassification::Sensitive
        );
        assert_eq!(
            classify("https://www.googleapis.com/auth/tasks"),
            ScopeClassification::NonSensitive
        );
    }

    #[test]
    fn document_scopes_filter_and_describe() {
        let doc: crate::discovery::RestDescription = serde_json::from_value(serde_json::json!({
            "name": "chat", "version": "v1", "rootUrl": "https://chat.googleapis.com/",
            "servicePath": "", "resources": {},
            "auth": {"oauth2": {"scopes": {
                "https://www.googleapis.com/auth/chat.bot": {"description": "bot"},
                "https://www.googleapis.com/auth/chat.spaces.readonly": {"description": ""},
                "https://www.google.com/m8/feeds": {"description": "legacy"}
            }}}
        }))
        .unwrap();
        let s = scopes_from_document(&doc);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].short, "chat.spaces.readonly");
        assert_eq!(s[0].description, "Chat Spaces Readonly");
        assert!(s[0].is_readonly);
    }
}
