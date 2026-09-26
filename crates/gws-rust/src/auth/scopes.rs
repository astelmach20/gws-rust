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

//! OAuth scope sets, service-to-scope mapping and per-method scope selection.

use std::collections::HashSet;

/// Prefix shared by almost all Google API scopes.
pub const SCOPE_PREFIX: &str = "https://www.googleapis.com/auth/";

/// Full Gmail access (the broadest Gmail scope).
pub const GMAIL_FULL_SCOPE: &str = "https://mail.google.com/";

/// Cross-service Google Cloud scope.
pub const PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Identity scopes always requested at login so the account can be shown and
/// Gmail helpers can read the display name.
pub const IDENTITY_SCOPES: &[&str] = &[
    "openid",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

/// Default login scopes: read-only access to the core Workspace APIs.
///
/// Least privilege by default. Add write access for the services you need
/// with `gwsr auth login --write -s <services>`; Google merges the new grant
/// with the existing one (incremental authorization).
pub const READONLY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/spreadsheets.readonly",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/calendar.readonly",
    "https://www.googleapis.com/auth/documents.readonly",
    "https://www.googleapis.com/auth/presentations.readonly",
    "https://www.googleapis.com/auth/tasks.readonly",
];

/// `--write`: read-write access to the core Workspace APIs.
pub const WRITE_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/spreadsheets",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/documents",
    "https://www.googleapis.com/auth/presentations",
    "https://www.googleapis.com/auth/tasks",
];

/// `--full`: read-write core scopes plus Gmail settings, Pub/Sub and Cloud
/// Platform. Unverified apps get a warning screen for several of these.
pub const FULL_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/spreadsheets",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/gmail.settings.basic",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/documents",
    "https://www.googleapis.com/auth/presentations",
    "https://www.googleapis.com/auth/tasks",
    "https://www.googleapis.com/auth/pubsub",
    PLATFORM_SCOPE,
];

/// Scopes that only grant access to a subset of a service's data or whose
/// mere presence restricts API behaviour (e.g. `gmail.metadata` disables the
/// `q` search parameter). They are chosen for a method only when no other
/// granted scope satisfies it.
const PARTIAL_VISIBILITY_SCOPES: &[&str] = &[
    "gmail.metadata",
    "gmail.addons.current.message.metadata",
    "gmail.addons.current.message.readonly",
    "gmail.addons.current.message.action",
    "gmail.addons.current.action.compose",
    "drive.file",
    "drive.appdata",
    "drive.appfolder",
    "drive.apps.readonly",
    "drive.install",
    "drive.meet.readonly",
    "drive.metadata",
    "drive.metadata.readonly",
    "drive.photos.readonly",
    "drive.scripts",
    "calendar.app.created",
    "calendar.events.owned",
    "calendar.events.owned.readonly",
    "calendar.events.freebusy",
    "calendar.freebusy",
    "calendar.events.public.readonly",
    "calendar.calendarlist",
    "calendar.calendarlist.readonly",
    "calendar.calendars",
    "calendar.calendars.readonly",
];

/// The short name of a scope (`drive.readonly` for `.../auth/drive.readonly`).
pub fn short_name(scope: &str) -> &str {
    scope.strip_prefix(SCOPE_PREFIX).unwrap_or(scope)
}

/// Expand a user-supplied scope: `drive.readonly` becomes
/// `https://www.googleapis.com/auth/drive.readonly`; full URLs and the OpenID
/// scopes (`openid`, `email`, `profile`) are kept as-is.
pub fn normalize_scope(input: &str) -> String {
    let s = input.trim();
    if s.contains("://") || matches!(s, "openid" | "email" | "profile") {
        s.to_string()
    } else {
        format!("{SCOPE_PREFIX}{s}")
    }
}

fn is_partial_visibility(scope: &str) -> bool {
    PARTIAL_VISIBILITY_SCOPES.contains(&short_name(scope))
}

fn is_readonly(scope: &str) -> bool {
    short_name(scope).ends_with("readonly")
}

/// Breadth rank: fewer dotted segments is broader (a smaller number). `https://mail.google.com/`
/// and `cloud-platform` are the broadest.
fn breadth(scope: &str) -> usize {
    if scope == GMAIL_FULL_SCOPE || scope == PLATFORM_SCOPE {
        return 0;
    }
    short_name(scope).split('.').count()
}

/// How the scopes for an API call were chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeChoice {
    /// A granted scope that satisfies the method.
    Granted(String),
    /// Granted scopes are unknown (external credentials, env token, ADC); the
    /// method's first listed scope is requested.
    Default(String),
    /// Granted scopes are known but none satisfies the method. The first listed
    /// scope is requested and the call is expected to fail with a 403.
    NotGranted {
        requested: String,
        required_any_of: Vec<String>,
        granted: Vec<String>,
    },
    /// The method declares no scopes.
    None,
}

impl ScopeChoice {
    /// The scopes to request a token for.
    pub fn scopes(&self) -> Vec<&str> {
        match self {
            ScopeChoice::Granted(s) | ScopeChoice::Default(s) => vec![s.as_str()],
            ScopeChoice::NotGranted { requested, .. } => vec![requested.as_str()],
            ScopeChoice::None => Vec::new(),
        }
    }
}

/// Choose the scope to request for a Discovery method.
///
/// `method_scopes` are alternatives (any one grants access). When the scopes
/// the user granted are known, pick among the granted alternatives:
///
/// 1. drop partial-visibility scopes (`drive.file`, `gmail.metadata`, ...)
///    unless nothing else remains, because they silently hide data;
/// 2. for `GET` methods prefer a read-only scope (least privilege, same data);
///    for other methods prefer a read-write scope;
/// 3. then prefer the narrowest scope (most dotted segments), then the
///    Discovery order.
///
/// With unknown grants the method's first scope is used. When grants are
/// known but none match, the result says so, so the caller can warn loudly.
pub fn select_for_method(
    method_scopes: &[String],
    http_method: &str,
    granted: Option<&[String]>,
) -> ScopeChoice {
    let Some(first) = method_scopes.first() else {
        return ScopeChoice::None;
    };
    let Some(granted) = granted.filter(|g| !g.is_empty()) else {
        return ScopeChoice::Default(first.clone());
    };
    let granted_set: HashSet<&str> = granted.iter().map(String::as_str).collect();
    let candidates: Vec<(usize, &String)> = method_scopes
        .iter()
        .enumerate()
        .filter(|(_, s)| granted_set.contains(s.as_str()))
        .collect();
    if candidates.is_empty() {
        return ScopeChoice::NotGranted {
            requested: first.clone(),
            required_any_of: method_scopes.to_vec(),
            granted: granted.to_vec(),
        };
    }
    let full_visibility: Vec<(usize, &String)> = candidates
        .iter()
        .copied()
        .filter(|(_, s)| !is_partial_visibility(s))
        .collect();
    let pool = if full_visibility.is_empty() {
        candidates
    } else {
        full_visibility
    };
    let want_readonly = http_method.eq_ignore_ascii_case("GET");
    pool.into_iter()
        .min_by_key(|(idx, s)| {
            (
                is_readonly(s) != want_readonly,
                std::cmp::Reverse(breadth(s)),
                *idx,
            )
        })
        .map(|(_, s)| ScopeChoice::Granted(s.clone()))
        .unwrap_or_else(|| ScopeChoice::Default(first.clone()))
}

/// Build a `gwsr auth login` command line that adds `scopes` to the current
/// grant (login uses incremental authorization).
pub fn login_command_hint(scopes: &[String]) -> String {
    let list = scopes
        .iter()
        .map(|s| short_name(s).to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("gwsr auth login --scopes {list}")
}

/// Map a user-facing service name to OAuth scope short-name prefixes.
pub fn service_scope_prefixes(service: &str) -> Vec<&str> {
    match service {
        "sheets" => vec!["spreadsheets"],
        "slides" => vec!["presentations"],
        "docs" => vec!["documents"],
        "people" => vec!["contacts", "directory"],
        "meet" => vec!["meetings"],
        "admin" => vec!["admin"],
        "admin-reports" | "reports" => vec!["admin.reports"],
        "directory" | "admin-directory" => vec!["admin.directory"],
        "alertcenter" => vec!["apps.alerts"],
        "groupssettings" => vec!["apps.groups.settings"],
        "licensing" => vec!["apps.licensing"],
        "reseller" => vec!["apps.order"],
        "vault" => vec!["ediscovery"],
        "cloudidentity" => vec!["cloud-identity"],
        "events" => vec!["chat", "meetings", "drive"],
        "script" => vec!["script"],
        s => vec![s],
    }
}

/// Check that every `--services` name is a known service (an alias such as
/// `gmail` or `sheets`, or `<api>:<version>`).
///
/// # Errors
///
/// [`crate::error::GwsError::Validation`] naming the first unknown entry, so a
/// typo or a scope name (`calendar.events`) is never silently dropped.
pub fn validate_service_names(services: &HashSet<String>) -> Result<(), crate::error::GwsError> {
    let mut names: Vec<&String> = services.iter().collect();
    names.sort();
    for name in names {
        crate::services::resolve_service(name).map_err(|e| {
            crate::error::GwsError::Validation(format!(
                "--services: '{name}' is not a service name ({e}). Pass exact scopes with \
                 --scopes instead"
            ))
        })?;
    }
    Ok(())
}

/// Whether `scope_url` belongs to one of `services`.
///
/// `https://mail.google.com/` belongs to `gmail`. `cloud-platform` belongs to
/// none: it grants access to every Google Cloud resource, so it is only
/// requested when asked for explicitly (`--full` without `--services`,
/// `--scopes`, or its own row in the picker).
pub fn scope_matches_service(scope_url: &str, services: &HashSet<String>) -> bool {
    if scope_url == PLATFORM_SCOPE {
        return false;
    }
    if scope_url == GMAIL_FULL_SCOPE {
        return services.contains("gmail");
    }
    let short = short_name(scope_url);
    services.iter().any(|svc| {
        service_scope_prefixes(svc)
            .iter()
            .any(|p| short == *p || short.starts_with(&format!("{p}.")))
    })
}

/// Keep only scopes belonging to `services` (all scopes when `None`/empty).
pub fn filter_scopes_by_services(
    scopes: Vec<String>,
    services: Option<&HashSet<String>>,
) -> Vec<String> {
    match services {
        Some(s) if !s.is_empty() => scopes
            .into_iter()
            .filter(|scope| scope_matches_service(scope, s))
            .collect(),
        _ => scopes,
    }
}

/// Services in `services` with no matching scope in `scopes`
/// (`cloud-platform` counts for none of them).
pub fn find_unmatched_services(scopes: &[String], services: &HashSet<String>) -> HashSet<String> {
    services
        .iter()
        .filter(|svc| {
            let one: HashSet<String> = std::iter::once((*svc).clone()).collect();
            !scopes.iter().any(|s| scope_matches_service(s, &one))
        })
        .cloned()
        .collect()
}

/// Remove scopes whose presence restricts API behaviour when a broader
/// alternative is also requested (`gmail.metadata` blocks `q` even alongside
/// `gmail.readonly`).
pub fn filter_redundant_restrictive_scopes(scopes: Vec<String>) -> Vec<String> {
    const RESTRICTIVE: &[(&str, &[&str])] = &[(
        "https://www.googleapis.com/auth/gmail.metadata",
        &[
            GMAIL_FULL_SCOPE,
            "https://www.googleapis.com/auth/gmail.modify",
            "https://www.googleapis.com/auth/gmail.readonly",
        ],
    )];
    let set: HashSet<String> = scopes.iter().cloned().collect();
    scopes
        .into_iter()
        .filter(|scope| {
            !RESTRICTIVE.iter().any(|(restrictive, broader)| {
                scope == restrictive && broader.iter().any(|b| set.contains(*b))
            })
        })
        .collect()
}

/// Drop scopes subsumed by a broader selected scope (`drive.readonly` when
/// `drive` is present), preserving order.
pub fn dedup_hierarchical(scopes: &[String]) -> Vec<String> {
    let shorts: Vec<&str> = scopes
        .iter()
        .filter_map(|s| s.strip_prefix(SCOPE_PREFIX))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for scope in scopes {
        if let Some(short) = scope.strip_prefix(SCOPE_PREFIX)
            && is_subsumed(short, &shorts)
        {
            continue;
        }
        if !out.contains(scope) {
            out.push(scope.clone());
        }
    }
    out
}

/// Whether `short` is a dotted child of another short name in `all`.
pub fn is_subsumed(short: &str, all: &[&str]) -> bool {
    all.iter().any(|&other| {
        other != short
            && short.starts_with(other)
            && short.as_bytes().get(other.len()) == Some(&b'.')
    })
}

/// Scopes that cannot be granted through a user OAuth consent (they need a
/// Chat app identity).
pub fn is_app_only_scope(url: &str) -> bool {
    let short = short_name(url);
    short.starts_with("chat.app.") || short == "chat.bot" || short.starts_with("chat.import")
}

/// Scopes that need a Workspace administrator (or an admin-enabled API) and
/// fail with `invalid_scope` for personal accounts. They are selectable but
/// never part of a preset.
pub fn is_workspace_admin_scope(url: &str) -> bool {
    let short = short_name(url);
    short.starts_with("admin.")
        || short.starts_with("apps.")
        || short.starts_with("cloud-identity.")
        || short.starts_with("chat.admin.")
        || short.starts_with("classroom.")
        || short == "keep"
        || short == "keep.readonly"
        || short == "ediscovery"
        || short == "ediscovery.readonly"
        || short == "directory.readonly"
        || short == "groups"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn a(short: &str) -> String {
        format!("{SCOPE_PREFIX}{short}")
    }

    fn gmail_list_scopes() -> Vec<String> {
        // Order as in the Gmail Discovery document for users.messages.list.
        vec![
            GMAIL_FULL_SCOPE.to_string(),
            a("gmail.metadata"),
            a("gmail.modify"),
            a("gmail.readonly"),
        ]
    }

    #[test]
    fn default_login_scopes_are_readonly() {
        assert!(READONLY_SCOPES.iter().all(|s| s.ends_with(".readonly")));
    }

    #[test]
    fn readonly_grant_picks_gmail_readonly_for_messages_list() {
        let granted = s(&["openid", &a("gmail.readonly"), &a("userinfo.email")]);
        let choice = select_for_method(&gmail_list_scopes(), "GET", Some(&granted));
        assert_eq!(choice, ScopeChoice::Granted(a("gmail.readonly")));
    }

    #[test]
    fn broad_grant_still_prefers_readonly_for_get() {
        let granted = s(&[GMAIL_FULL_SCOPE, &a("gmail.modify"), &a("gmail.readonly")]);
        let choice = select_for_method(&gmail_list_scopes(), "GET", Some(&granted));
        assert_eq!(choice, ScopeChoice::Granted(a("gmail.readonly")));
    }

    #[test]
    fn write_method_prefers_write_scope() {
        // gmail.users.messages.send
        let method = s(&[
            GMAIL_FULL_SCOPE,
            &a("gmail.addons.current.action.compose"),
            &a("gmail.compose"),
            &a("gmail.modify"),
            &a("gmail.send"),
        ]);
        let granted = s(&[GMAIL_FULL_SCOPE, &a("gmail.modify"), &a("gmail.readonly")]);
        let choice = select_for_method(&method, "POST", Some(&granted));
        assert_eq!(choice, ScopeChoice::Granted(a("gmail.modify")));

        let granted = s(&[GMAIL_FULL_SCOPE]);
        let choice = select_for_method(&method, "POST", Some(&granted));
        assert_eq!(choice, ScopeChoice::Granted(GMAIL_FULL_SCOPE.to_string()));
    }

    #[test]
    fn metadata_only_grant_falls_back_to_partial_scope() {
        let granted = s(&[&a("gmail.metadata")]);
        let choice = select_for_method(&gmail_list_scopes(), "GET", Some(&granted));
        assert_eq!(choice, ScopeChoice::Granted(a("gmail.metadata")));
    }

    #[test]
    fn partial_visibility_scopes_are_avoided() {
        let method = s(&[
            &a("drive"),
            &a("drive.appdata"),
            &a("drive.file"),
            &a("drive.meet.readonly"),
            &a("drive.metadata.readonly"),
            &a("drive.photos.readonly"),
            &a("drive.readonly"),
        ]);
        let granted = s(&[
            &a("drive.file"),
            &a("drive.readonly"),
            &a("drive.photos.readonly"),
        ]);
        assert_eq!(
            select_for_method(&method, "GET", Some(&granted)),
            ScopeChoice::Granted(a("drive.readonly"))
        );
    }

    #[test]
    fn unknown_grants_use_first_scope() {
        assert_eq!(
            select_for_method(&gmail_list_scopes(), "GET", None),
            ScopeChoice::Default(GMAIL_FULL_SCOPE.to_string())
        );
        assert_eq!(
            select_for_method(&gmail_list_scopes(), "GET", Some(&[])),
            ScopeChoice::Default(GMAIL_FULL_SCOPE.to_string())
        );
    }

    #[test]
    fn no_intersection_is_reported() {
        let granted = s(&[&a("drive.readonly")]);
        match select_for_method(&gmail_list_scopes(), "GET", Some(&granted)) {
            ScopeChoice::NotGranted {
                requested,
                required_any_of,
                granted: g,
            } => {
                assert_eq!(requested, GMAIL_FULL_SCOPE);
                assert_eq!(required_any_of, gmail_list_scopes());
                assert_eq!(g, granted);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn no_method_scopes() {
        assert_eq!(select_for_method(&[], "GET", None), ScopeChoice::None);
        assert!(ScopeChoice::None.scopes().is_empty());
    }

    #[test]
    fn normalize_and_hint() {
        assert_eq!(normalize_scope(" drive.readonly "), a("drive.readonly"));
        assert_eq!(normalize_scope(GMAIL_FULL_SCOPE), GMAIL_FULL_SCOPE);
        assert_eq!(normalize_scope("openid"), "openid");
        assert_eq!(
            login_command_hint(&[a("gmail.modify"), GMAIL_FULL_SCOPE.to_string()]),
            "gwsr auth login --scopes gmail.modify,https://mail.google.com/"
        );
    }

    #[test]
    fn service_mapping() {
        let set = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<HashSet<_>>();
        assert!(scope_matches_service(
            &a("meetings.space.readonly"),
            &set(&["meet"])
        ));
        assert!(scope_matches_service(&a("keep.readonly"), &set(&["keep"])));
        assert!(scope_matches_service(
            &a("apps.alerts"),
            &set(&["alertcenter"])
        ));
        assert!(scope_matches_service(
            &a("admin.reports.audit.readonly"),
            &set(&["reports"])
        ));
        assert!(scope_matches_service(
            &a("admin.directory.user"),
            &set(&["admin"])
        ));
        assert!(scope_matches_service(
            &a("gmail.settings.basic"),
            &set(&["gmail"])
        ));
        assert!(scope_matches_service(GMAIL_FULL_SCOPE, &set(&["gmail"])));
        assert!(!scope_matches_service(GMAIL_FULL_SCOPE, &set(&["drive"])));
        assert!(scope_matches_service(&a("spreadsheets"), &set(&["sheets"])));
        assert!(scope_matches_service(
            &a("contacts.readonly"),
            &set(&["people"])
        ));
        // cloud-platform reaches every Google Cloud resource; it belongs to no
        // Workspace service, so `-s drive` must not keep it.
        assert!(!scope_matches_service(PLATFORM_SCOPE, &set(&["drive"])));
        assert!(!scope_matches_service(PLATFORM_SCOPE, &set(&["gmail"])));
        assert!(!scope_matches_service(&a("drivelabels"), &set(&["drive"])));
    }

    #[test]
    fn service_names_are_validated() {
        let set = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<HashSet<_>>();
        assert!(validate_service_names(&set(&["gmail", "drive", "sheets", "chat"])).is_ok());
        let err = validate_service_names(&set(&["gmail", "contacts.other.readonly"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("contacts.other.readonly"), "{err}");
        assert!(validate_service_names(&set(&["calendar.events"])).is_err());
    }

    #[test]
    fn unmatched_services() {
        let set: HashSet<String> = ["drive", "meet"].iter().map(|x| x.to_string()).collect();
        let missing = find_unmatched_services(&s(&[&a("drive"), PLATFORM_SCOPE]), &set);
        assert_eq!(missing, ["meet".to_string()].into_iter().collect());
    }

    #[test]
    fn restrictive_and_hierarchical_filters() {
        let out =
            filter_redundant_restrictive_scopes(s(&[&a("gmail.metadata"), &a("gmail.readonly")]));
        assert_eq!(out, s(&[&a("gmail.readonly")]));
        let out = filter_redundant_restrictive_scopes(s(&[&a("gmail.metadata")]));
        assert_eq!(out, s(&[&a("gmail.metadata")]));

        let out = dedup_hierarchical(&s(&[
            &a("drive.readonly"),
            &a("drive"),
            &a("drive"),
            "openid",
        ]));
        assert_eq!(out, s(&[&a("drive"), "openid"]));
    }

    #[test]
    fn app_only_and_admin_classification() {
        assert!(is_app_only_scope(&a("chat.bot")));
        assert!(is_app_only_scope(&a("chat.app.spaces")));
        assert!(!is_app_only_scope(&a("keep")), "keep is usable by users");
        assert!(!is_app_only_scope(&a("apps.alerts")));
        assert!(is_workspace_admin_scope(&a("apps.alerts")));
        assert!(is_workspace_admin_scope(&a("keep")));
        assert!(is_workspace_admin_scope(&a("admin.directory.user")));
        assert!(!is_workspace_admin_scope(&a("drive")));
    }
}
