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

//! Interactive scope picker for `gwsr auth login` and Discovery-based scope
//! lookup for services that have no preset scopes.

use std::collections::HashSet;

use crate::auth::scopes::{self, PLATFORM_SCOPE};
use crate::auth::setup::apis::{DiscoveredScope, ScopeClassification};
use crate::auth::setup::tui::{PickerResult, SelectItem};
use crate::error::GwsError;

const TEMPLATE_READONLY: usize = 0;
const TEMPLATE_WRITE: usize = 1;
const TEMPLATE_FULL: usize = 2;
const TEMPLATE_COUNT: usize = 3;
const MANY_SCOPES_WARNING: usize = 25;

/// Show the picker. `Ok(None)` means no picker could be shown (nothing to
/// pick, or the terminal UI failed, reported on stderr); the caller then uses
/// the read-only defaults. Cancelling or selecting nothing is an error.
pub(super) async fn pick(
    project_id: Option<&str>,
    services: Option<&HashSet<String>>,
) -> Result<Option<Vec<String>>, GwsError> {
    let discovered = match project_id {
        Some(pid) => match crate::auth::setup::gcloud::get_enabled_apis(pid).await {
            Ok(apis) if !apis.is_empty() => {
                crate::auth::setup::apis::fetch_scopes_for_apis(&apis).await
            }
            Ok(_) => Vec::new(),
            Err(e) => {
                eprintln!(
                    "note: could not list enabled APIs for project '{pid}' ({e:#}); showing the \
                     basic scope list"
                );
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    let entries: Vec<DiscoveredScope> = if discovered.is_empty() {
        static_entries()
    } else {
        discovered
    };
    run_picker(&entries, services)
}

/// Scope entries used when Discovery-based lookup is unavailable.
fn static_entries() -> Vec<DiscoveredScope> {
    let mut all: Vec<&str> = scopes::READONLY_SCOPES.to_vec();
    all.extend_from_slice(scopes::FULL_SCOPES);
    all.into_iter()
        .map(|url| {
            let short = scopes::short_name(url).to_string();
            DiscoveredScope {
                url: url.to_string(),
                description: url.to_string(),
                is_readonly: short.ends_with("readonly"),
                classification: crate::auth::setup::apis::classify(url),
                short,
            }
        })
        .collect()
}

/// Selections implied by each template, as short names.
pub(super) struct Templates {
    pub readonly: Vec<String>,
    pub write: Vec<String>,
    pub full: Vec<String>,
}

pub(super) fn templates(entries: &[&DiscoveredScope]) -> Templates {
    let shorts: Vec<&str> = entries.iter().map(|e| e.short.as_str()).collect();
    let in_preset = |preset: &[&str], e: &DiscoveredScope| preset.contains(&e.url.as_str());
    let mut t = Templates {
        readonly: Vec::new(),
        write: Vec::new(),
        full: Vec::new(),
    };
    for e in entries {
        if in_preset(scopes::READONLY_SCOPES, e) {
            t.readonly.push(e.short.clone());
        }
        if in_preset(scopes::WRITE_SCOPES, e) {
            t.write.push(e.short.clone());
        }
        if !scopes::is_workspace_admin_scope(&e.url) && !scopes::is_subsumed(&e.short, &shorts) {
            t.full.push(e.short.clone());
        }
    }
    t
}

fn run_picker(
    entries: &[DiscoveredScope],
    services: Option<&HashSet<String>>,
) -> Result<Option<Vec<String>>, GwsError> {
    let filtered: Vec<&DiscoveredScope> = entries
        .iter()
        .filter(|e| !scopes::is_app_only_scope(&e.url))
        .filter(|e| {
            services.is_none_or(|s| s.is_empty() || scopes::scope_matches_service(&e.url, s))
        })
        .collect();
    if filtered.is_empty() {
        return Ok(None);
    }
    let t = templates(&filtered);
    let mut items = vec![
        template_item(
            "Read only (recommended)",
            "Read-only access to the core services",
            true,
            t.readonly.clone(),
        ),
        template_item(
            "Read/write",
            "Read-write access to the core services",
            false,
            t.write,
        ),
        template_item(
            "Full access",
            "Every listed scope except admin-only ones (unverified apps: max ~25 scopes)",
            false,
            t.full,
        ),
    ];
    for e in &filtered {
        let tag = match e.classification {
            ScopeClassification::Restricted => "RESTRICTED ",
            ScopeClassification::Sensitive => "SENSITIVE ",
            ScopeClassification::NonSensitive => "",
        };
        let admin = if scopes::is_workspace_admin_scope(&e.url) {
            "ADMIN "
        } else {
            ""
        };
        items.push(SelectItem {
            label: e.short.clone(),
            description: format!("{admin}{tag}{}", e.description),
            selected: t.readonly.contains(&e.short),
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        });
    }

    let result = match crate::auth::setup::tui::run_picker(
        "Select OAuth scopes",
        "Space to toggle, Enter to confirm",
        items,
        true,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: the scope picker failed ({e}); using the read-only defaults");
            return Ok(None);
        }
    };
    let PickerResult::Confirmed(items) = result else {
        return Err(GwsError::Validation("login cancelled".into()));
    };
    let chosen = selected_from_items(&items, &filtered);
    if chosen.len() > MANY_SCOPES_WARNING {
        eprintln!(
            "warning: {} scopes selected; unverified OAuth apps may be refused with this many",
            chosen.len()
        );
    }
    if chosen.is_empty() {
        return Err(GwsError::Validation(
            "no scopes selected; login cancelled".into(),
        ));
    }
    Ok(Some(chosen))
}

fn template_item(label: &str, desc: &str, selected: bool, selects: Vec<String>) -> SelectItem {
    SelectItem {
        label: label.to_string(),
        description: desc.to_string(),
        selected,
        is_fixed: false,
        is_template: true,
        template_selects: selects,
    }
}

/// Map confirmed picker items back to scope URLs.
pub(super) fn selected_from_items(
    items: &[SelectItem],
    entries: &[&DiscoveredScope],
) -> Vec<String> {
    let template_selected = |i: usize| items.get(i).is_some_and(|it| it.selected);
    let full = template_selected(TEMPLATE_FULL);
    let mut chosen: Vec<String> = items
        .iter()
        .skip(TEMPLATE_COUNT)
        .zip(entries.iter())
        .filter(|(item, _)| item.selected)
        .map(|(_, e)| e.url.clone())
        .collect();
    if full
        && !template_selected(TEMPLATE_READONLY)
        && !template_selected(TEMPLATE_WRITE)
        && !chosen.iter().any(|s| s == PLATFORM_SCOPE)
    {
        chosen.push(PLATFORM_SCOPE.to_string());
    }
    scopes::dedup_hierarchical(&chosen)
}

/// For services with no preset scope, add their scopes from Discovery.
///
/// Services whose Discovery document cannot be fetched are reported on
/// stderr; the login continues with the scopes that are known.
pub(crate) async fn augment_with_discovery_scopes(
    result: &mut Vec<String>,
    services: &HashSet<String>,
    readonly_only: bool,
) {
    let missing = scopes::find_unmatched_services(result, services);
    if missing.is_empty() {
        return;
    }
    let mut names: Vec<&String> = missing.iter().collect();
    names.sort();
    let lookups = names.into_iter().map(|svc| async move {
        let found = async {
            let (api, version) =
                crate::services::resolve_service(svc).map_err(|e| anyhow::anyhow!("{e}"))?;
            let doc = crate::discovery::fetch_discovery_document(&api, &version).await?;
            anyhow::Ok(scopes_from_doc(&doc, readonly_only))
        }
        .await;
        (svc, found)
    });
    for (svc, found) in futures_util::future::join_all(lookups).await {
        match found {
            Ok(list) if !list.is_empty() => {
                for s in list {
                    if !result.contains(&s) {
                        result.push(s);
                    }
                }
            }
            Ok(_) => eprintln!(
                "warning: service '{svc}' has no {}OAuth scopes usable for a user login",
                if readonly_only { "read-only " } else { "" }
            ),
            Err(e) => eprintln!("warning: no scopes added for service '{svc}': {e:#}"),
        }
    }
}

/// OAuth scopes declared by a Discovery document (user-grantable only).
pub(crate) fn scopes_from_doc(
    doc: &crate::discovery::RestDescription,
    readonly_only: bool,
) -> Vec<String> {
    let Some(map) = doc
        .auth
        .as_ref()
        .and_then(|a| a.oauth2.as_ref())
        .and_then(|o| o.scopes.as_ref())
    else {
        return Vec::new();
    };
    let mut v: Vec<String> = map
        .keys()
        .filter(|url| !scopes::is_app_only_scope(url))
        .filter(|url| !readonly_only || url.ends_with("readonly"))
        .cloned()
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(short: &str) -> DiscoveredScope {
        let url = format!("{}{short}", scopes::SCOPE_PREFIX);
        DiscoveredScope {
            classification: crate::auth::setup::apis::classify(&url),
            url,
            short: short.to_string(),
            description: String::new(),
            is_readonly: short.ends_with("readonly"),
        }
    }

    #[test]
    fn templates_follow_presets_and_skip_admin_scopes() {
        let entries = [
            entry("drive"),
            entry("drive.readonly"),
            entry("drive.file"),
            entry("gmail.modify"),
            entry("gmail.readonly"),
            entry("admin.directory.user"),
        ];
        let refs: Vec<&DiscoveredScope> = entries.iter().collect();
        let t = templates(&refs);
        assert_eq!(t.readonly, vec!["drive.readonly", "gmail.readonly"]);
        assert_eq!(t.write, vec!["drive", "gmail.modify"]);
        assert_eq!(t.full, vec!["drive", "gmail.modify", "gmail.readonly"]);
    }

    #[test]
    fn selection_maps_items_to_urls_and_dedups() {
        let entries = [
            entry("drive"),
            entry("drive.readonly"),
            entry("gmail.readonly"),
        ];
        let refs: Vec<&DiscoveredScope> = entries.iter().collect();
        let mut items = vec![
            template_item("r", "", false, vec![]),
            template_item("w", "", false, vec![]),
            template_item("f", "", true, vec![]),
        ];
        for (i, e) in entries.iter().enumerate() {
            items.push(SelectItem {
                label: e.short.clone(),
                description: String::new(),
                selected: i != 2,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            });
        }
        let chosen = selected_from_items(&items, &refs);
        assert_eq!(
            chosen,
            vec![
                format!("{}drive", scopes::SCOPE_PREFIX),
                PLATFORM_SCOPE.to_string()
            ]
        );
    }

    #[test]
    fn static_entries_cover_presets() {
        let e = static_entries();
        assert!(e.iter().any(|x| x.short == "gmail.settings.basic"));
        assert!(e.iter().any(|x| x.short == "drive.readonly"));
    }

    #[test]
    fn scopes_from_doc_filters() {
        let doc: crate::discovery::RestDescription = serde_json::from_value(serde_json::json!({
            "name": "chat", "version": "v1", "rootUrl": "https://chat.googleapis.com/",
            "servicePath": "", "resources": {},
            "auth": {"oauth2": {"scopes": {
                "https://www.googleapis.com/auth/chat.bot": {"description": "bot"},
                "https://www.googleapis.com/auth/chat.messages": {"description": "m"},
                "https://www.googleapis.com/auth/chat.messages.readonly": {"description": "r"}
            }}}
        }))
        .unwrap();
        assert_eq!(
            scopes_from_doc(&doc, false),
            vec![
                "https://www.googleapis.com/auth/chat.messages".to_string(),
                "https://www.googleapis.com/auth/chat.messages.readonly".to_string()
            ]
        );
        assert_eq!(
            scopes_from_doc(&doc, true),
            vec!["https://www.googleapis.com/auth/chat.messages.readonly".to_string()]
        );
    }
}
