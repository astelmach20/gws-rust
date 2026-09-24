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

//! Google Workspace service registry.
//!
//! Maps service aliases (e.g. "drive", "gmail") to Discovery API names and
//! versions, and resolves user-supplied service specs of the form `alias`,
//! `alias:version`, or `<api>:<version>` for any Discovery API.

use crate::error::GwsError;
use crate::validate::validate_api_identifier;

/// A known service with its alias, API name, version, and description.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceEntry {
    pub aliases: &'static [&'static str],
    pub api_name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

/// All known services with metadata.
pub const SERVICES: &[ServiceEntry] = &[
    ServiceEntry {
        aliases: &["drive"],
        api_name: "drive",
        version: "v3",
        description: "Manage files, folders, and shared drives",
    },
    ServiceEntry {
        aliases: &["sheets"],
        api_name: "sheets",
        version: "v4",
        description: "Read and write spreadsheets",
    },
    ServiceEntry {
        aliases: &["gmail"],
        api_name: "gmail",
        version: "v1",
        description: "Send, read, and manage email",
    },
    ServiceEntry {
        aliases: &["calendar"],
        api_name: "calendar",
        version: "v3",
        description: "Manage calendars and events",
    },
    ServiceEntry {
        aliases: &["admin-reports", "reports"],
        api_name: "admin",
        version: "reports_v1",
        description: "Audit logs and usage reports",
    },
    ServiceEntry {
        aliases: &["admin", "directory", "admin-directory"],
        api_name: "admin",
        version: "directory_v1",
        description: "Manage users, groups, org units, devices, and roles (Admin SDK Directory)",
    },
    ServiceEntry {
        aliases: &["datatransfer", "admin-datatransfer"],
        api_name: "admin",
        version: "datatransfer_v1",
        description: "Transfer user data between accounts (Admin SDK Data Transfer)",
    },
    ServiceEntry {
        aliases: &["alertcenter"],
        api_name: "alertcenter",
        version: "v1beta1",
        description: "Manage Workspace security alerts",
    },
    ServiceEntry {
        aliases: &["cloudidentity"],
        api_name: "cloudidentity",
        version: "v1",
        description: "Manage identity groups, memberships, and devices",
    },
    ServiceEntry {
        aliases: &["groupssettings"],
        api_name: "groupssettings",
        version: "v1",
        description: "Manage Google Groups settings",
    },
    ServiceEntry {
        aliases: &["licensing"],
        api_name: "licensing",
        version: "v1",
        description: "Assign and manage product licenses",
    },
    ServiceEntry {
        aliases: &["reseller"],
        api_name: "reseller",
        version: "v1",
        description: "Manage reseller customers and subscriptions",
    },
    ServiceEntry {
        aliases: &["vault"],
        api_name: "vault",
        version: "v1",
        description: "Manage eDiscovery matters, holds, and exports",
    },
    ServiceEntry {
        aliases: &["driveactivity"],
        api_name: "driveactivity",
        version: "v2",
        description: "Query activity on Drive files and folders",
    },
    ServiceEntry {
        aliases: &["drivelabels"],
        api_name: "drivelabels",
        version: "v2",
        description: "Manage Drive labels and classification",
    },
    ServiceEntry {
        aliases: &["chromemanagement"],
        api_name: "chromemanagement",
        version: "v1",
        description: "Chrome browser and ChromeOS device reports and telemetry",
    },
    ServiceEntry {
        aliases: &["chromepolicy"],
        api_name: "chromepolicy",
        version: "v1",
        description: "Manage Chrome policies for users and devices",
    },
    ServiceEntry {
        aliases: &["postmaster", "gmailpostmastertools"],
        api_name: "gmailpostmastertools",
        version: "v2",
        description: "Gmail sender reputation and delivery metrics (Postmaster Tools)",
    },
    ServiceEntry {
        aliases: &["cloudsearch"],
        api_name: "cloudsearch",
        version: "v1",
        description: "Manage Cloud Search data sources and queries",
    },
    ServiceEntry {
        aliases: &["docs"],
        api_name: "docs",
        version: "v1",
        description: "Read and write Google Docs",
    },
    ServiceEntry {
        aliases: &["slides"],
        api_name: "slides",
        version: "v1",
        description: "Read and write presentations",
    },
    ServiceEntry {
        aliases: &["tasks"],
        api_name: "tasks",
        version: "v1",
        description: "Manage task lists and tasks",
    },
    ServiceEntry {
        aliases: &["people"],
        api_name: "people",
        version: "v1",
        description: "Manage contacts and profiles",
    },
    ServiceEntry {
        aliases: &["chat"],
        api_name: "chat",
        version: "v1",
        description: "Manage Chat spaces and messages",
    },
    ServiceEntry {
        aliases: &["classroom"],
        api_name: "classroom",
        version: "v1",
        description: "Manage classes, rosters, and coursework",
    },
    ServiceEntry {
        aliases: &["forms"],
        api_name: "forms",
        version: "v1",
        description: "Read and write Google Forms",
    },
    ServiceEntry {
        aliases: &["keep"],
        api_name: "keep",
        version: "v1",
        description: "Manage Google Keep notes",
    },
    ServiceEntry {
        aliases: &["meet"],
        api_name: "meet",
        version: "v2",
        description: "Manage Google Meet conferences",
    },
    ServiceEntry {
        aliases: &["events"],
        api_name: "workspaceevents",
        version: "v1",
        description: "Subscribe to Google Workspace events",
    },
    ServiceEntry {
        aliases: &["modelarmor"],
        api_name: "modelarmor",
        version: "v1",
        description: "Filter user-generated content for safety",
    },
    ServiceEntry {
        aliases: &["workflow", "wf"],
        api_name: "workflow",
        version: "v1",
        description: "Cross-service productivity workflows",
    },
    ServiceEntry {
        aliases: &["script"],
        api_name: "script",
        version: "v1",
        description: "Manage Google Apps Script projects",
    },
];

/// A resolved service: the Discovery API name and version to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedService {
    /// Discovery API name (e.g. `admin`, `drive`, `youtube`).
    pub api_name: String,
    /// Discovery API version (e.g. `directory_v1`, `v3`).
    pub version: String,
    /// The registry entry, when the spec named a registered alias.
    pub entry: Option<&'static ServiceEntry>,
}

/// Look up a registered service by alias.
pub fn find_service(alias: &str) -> Option<&'static ServiceEntry> {
    SERVICES.iter().find(|e| e.aliases.contains(&alias))
}

/// Resolves a service spec to a Discovery API name and version.
///
/// Accepted forms:
/// - `alias` — a registered service (e.g. `drive`, `admin`).
/// - `alias:version` — a registered service with a version override (e.g. `drive:v2`).
/// - `api:version` — any Discovery API, registered or not (e.g. `youtube:v3`).
///
/// `version_override` (e.g. from `--api-version`) wins over a version given in
/// the spec and, like `api:version`, allows unregistered APIs.
pub fn resolve_service_spec(
    spec: &str,
    version_override: Option<&str>,
) -> Result<ResolvedService, GwsError> {
    let (name, spec_version) = match spec.split_once(':') {
        Some((name, ver)) => {
            if ver.is_empty() {
                return Err(GwsError::Validation(format!(
                    "Service spec '{spec}' has an empty version; use '<api>:<version>', e.g. 'youtube:v3'"
                )));
            }
            (name, Some(ver))
        }
        None => (spec, None),
    };
    let version = version_override.or(spec_version);
    if let Some(v) = version {
        validate_api_identifier(v)?;
    }

    if let Some(entry) = find_service(name) {
        return Ok(ResolvedService {
            api_name: entry.api_name.to_string(),
            version: version.unwrap_or(entry.version).to_string(),
            entry: Some(entry),
        });
    }

    match version {
        Some(v) => {
            validate_api_identifier(name)?;
            Ok(ResolvedService {
                api_name: name.to_string(),
                version: v.to_string(),
                entry: None,
            })
        }
        None => Err(unknown_service_error(name)),
    }
}

/// Resolves a service spec (see [`resolve_service_spec`]) to `(api_name, version)`.
pub fn resolve_service(spec: &str) -> Result<(String, String), GwsError> {
    let resolved = resolve_service_spec(spec, None)?;
    Ok((resolved.api_name, resolved.version))
}

/// The registered alias closest to `name`, if any is plausibly a typo of it.
pub fn suggest_service(name: &str) -> Option<&'static str> {
    let lowered = name.to_ascii_lowercase();
    SERVICES
        .iter()
        .flat_map(|e| e.aliases.iter().copied())
        .map(|alias| (alias, strsim::jaro_winkler(&lowered, alias)))
        .filter(|(alias, score)| {
            *score >= 0.85 || strsim::damerau_levenshtein(&lowered, alias) <= 2 && alias.len() > 3
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(alias, _)| alias)
}

fn unknown_service_error(name: &str) -> GwsError {
    let all_names: Vec<&str> = SERVICES
        .iter()
        .flat_map(|e| e.aliases.iter().copied())
        .collect();
    let hint = match suggest_service(name) {
        Some(s) => format!(" Did you mean '{s}'?"),
        None => String::new(),
    };
    GwsError::Validation(format!(
        "Unknown service '{name}'.{hint} Known services: {}. \
         Any other Discovery API can be used as '<api>:<version>' (e.g. 'youtube:v3', 'admin:directory_v1').",
        all_names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn resolve_known_aliases() {
        assert_eq!(
            resolve_service("drive").unwrap(),
            ("drive".to_string(), "v3".to_string())
        );
        assert_eq!(
            resolve_service("reports").unwrap(),
            ("admin".to_string(), "reports_v1".to_string())
        );
        assert_eq!(
            resolve_service("admin").unwrap(),
            ("admin".to_string(), "directory_v1".to_string())
        );
        assert_eq!(
            resolve_service("postmaster").unwrap(),
            ("gmailpostmastertools".to_string(), "v2".to_string())
        );
        assert_eq!(
            resolve_service("datatransfer").unwrap(),
            ("admin".to_string(), "datatransfer_v1".to_string())
        );
    }

    #[test]
    fn restored_admin_and_enterprise_services_are_registered() {
        for (alias, api, ver) in [
            ("vault", "vault", "v1"),
            ("groupssettings", "groupssettings", "v1"),
            ("licensing", "licensing", "v1"),
            ("reseller", "reseller", "v1"),
            ("cloudidentity", "cloudidentity", "v1"),
            ("alertcenter", "alertcenter", "v1beta1"),
            ("driveactivity", "driveactivity", "v2"),
            ("drivelabels", "drivelabels", "v2"),
            ("chromemanagement", "chromemanagement", "v1"),
            ("chromepolicy", "chromepolicy", "v1"),
            ("cloudsearch", "cloudsearch", "v1"),
        ] {
            assert_eq!(
                resolve_service(alias).unwrap(),
                (api.to_string(), ver.to_string()),
                "{alias}"
            );
        }
    }

    #[test]
    fn aliases_are_unique_and_valid_identifiers() {
        let mut seen = HashSet::new();
        for entry in SERVICES {
            assert!(!entry.description.is_empty());
            validate_api_identifier(entry.api_name).unwrap();
            validate_api_identifier(entry.version).unwrap();
            for alias in entry.aliases {
                assert!(seen.insert(*alias), "duplicate alias {alias}");
                assert!(!alias.contains(':'));
            }
        }
    }

    #[test]
    fn alias_with_version_override() {
        let r = resolve_service_spec("drive:v2", None).unwrap();
        assert_eq!((r.api_name.as_str(), r.version.as_str()), ("drive", "v2"));
        assert!(r.entry.is_some());
        let r = resolve_service_spec("drive:v2", Some("v1")).unwrap();
        assert_eq!(r.version, "v1");
        let r = resolve_service_spec("reports:directory_v1", None).unwrap();
        assert_eq!(
            (r.api_name.as_str(), r.version.as_str()),
            ("admin", "directory_v1")
        );
    }

    #[test]
    fn any_discovery_api_with_version() {
        let r = resolve_service_spec("youtube:v3", None).unwrap();
        assert_eq!((r.api_name.as_str(), r.version.as_str()), ("youtube", "v3"));
        assert!(r.entry.is_none());
        let r = resolve_service_spec("youtube", Some("v3")).unwrap();
        assert_eq!((r.api_name.as_str(), r.version.as_str()), ("youtube", "v3"));
    }

    #[test]
    fn unregistered_api_names_are_validated() {
        for bad in ["../etc:v1", "you tube:v3", "yt?x=1:v3", ":v3", "..:v1"] {
            assert!(
                matches!(
                    resolve_service_spec(bad, None),
                    Err(GwsError::Validation(_))
                ),
                "{bad}"
            );
        }
        assert!(resolve_service_spec("youtube:v3/../x", None).is_err());
        assert!(resolve_service_spec("youtube:", None).is_err());
        assert!(resolve_service_spec("drive", Some("../v3")).is_err());
    }

    #[test]
    fn unknown_service_error_is_accurate() {
        let msg = resolve_service("unknown_service").unwrap_err().to_string();
        assert!(msg.contains("Unknown service 'unknown_service'"), "{msg}");
        assert!(msg.contains("'<api>:<version>'"), "{msg}");
        assert!(msg.contains("youtube:v3"), "{msg}");
    }

    #[test]
    fn did_you_mean_suggestions() {
        assert_eq!(suggest_service("drvie"), Some("drive"));
        assert_eq!(suggest_service("gmial"), Some("gmail"));
        assert_eq!(suggest_service("calender"), Some("calendar"));
        assert_eq!(suggest_service("Sheets"), Some("sheets"));
        assert_eq!(suggest_service("xyzzyqq"), None);
        let msg = resolve_service("drvie").unwrap_err().to_string();
        assert!(msg.contains("Did you mean 'drive'?"), "{msg}");
    }
}
