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

//! `gwsr commands`: an inventory of every command for humans and agents.
//!
//! The inventory is derived from the same clap trees the CLI parses with
//! (Discovery-generated resources plus injected `+helper` commands), so it
//! cannot drift from the real command surface.

use clap::Command;
use serde_json::{Value, json};

use crate::discovery::{RestDescription, RestResource};
use crate::error::GwsError;
use crate::services::{ResolvedService, SERVICES, ServiceEntry};

/// Describe one argument.
fn flag_json(arg: &clap::Arg) -> Value {
    let takes_value = arg.get_action().takes_values();
    let mut flag = json!({
        "name": arg.get_long().map(|l| format!("--{l}")).unwrap_or_else(|| arg.get_id().to_string()),
        "required": arg.is_required_set(),
        "takes_value": takes_value,
        "help": arg.get_help().map(|h| h.to_string()),
    });
    if let Some(short) = arg.get_short() {
        flag["short"] = json!(format!("-{short}"));
    }
    if takes_value {
        if let Some(names) = arg.get_value_names() {
            flag["value_name"] = json!(
                names
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        let possible: Vec<String> = arg
            .get_possible_values()
            .iter()
            .map(|p| p.get_name().to_string())
            .collect();
        if !possible.is_empty() {
            flag["possible_values"] = json!(possible);
        }
        if arg.get_long().is_none() && arg.get_short().is_none() {
            flag["positional"] = json!(true);
        }
    }
    flag
}

fn flags_of(cmd: &Command) -> Vec<Value> {
    cmd.get_arguments()
        .filter(|a| !a.is_global_set() && !matches!(a.get_id().as_str(), "help" | "version"))
        .map(flag_json)
        .collect()
}

fn find_resource<'a>(doc: &'a RestDescription, path: &[&str]) -> Option<&'a RestResource> {
    let (first, rest) = path.split_first()?;
    let mut res = doc.resources.get(*first)?;
    for name in rest {
        res = res.resources.get(*name)?;
    }
    Some(res)
}

/// Walk a service's command tree into flat command records.
fn walk(cmd: &Command, doc: &RestDescription, path: &mut Vec<String>, out: &mut Vec<Value>) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        path.push(name.clone());
        let is_leaf = sub.get_subcommands().next().is_none();
        if name.starts_with('+') {
            walk_helper(sub, path, out);
        } else if is_leaf {
            let parents: Vec<&str> = path[..path.len() - 1].iter().map(String::as_str).collect();
            let method = find_resource(doc, &parents).and_then(|r| r.methods.get(&name));
            out.push(json!({
                "path": path.join(" "),
                "kind": "method",
                "id": method.and_then(|m| m.id.clone()),
                "http_method": method.map(|m| m.http_method.clone()),
                "scopes": method.map(|m| m.scopes.clone()).unwrap_or_default(),
                "about": sub.get_about().map(|a| a.to_string()),
                "flags": flags_of(sub),
            }));
        } else {
            walk(sub, doc, path, out);
        }
        path.pop();
    }
}

/// Record a helper, or each runnable subcommand of a helper group such as
/// `gmail +filter create`.
fn walk_helper(cmd: &Command, path: &mut Vec<String>, out: &mut Vec<Value>) {
    if cmd.get_subcommands().next().is_none() {
        out.push(json!({
            "path": path.join(" "),
            "kind": "helper",
            "about": cmd.get_about().map(|a| a.to_string()),
            "flags": flags_of(cmd),
        }));
        return;
    }
    for sub in cmd.get_subcommands() {
        path.push(sub.get_name().to_string());
        walk_helper(sub, path, out);
        path.pop();
    }
}

/// Inventory for one registered service.
pub fn service_inventory(entry: &ServiceEntry, doc: &RestDescription) -> Value {
    let cmd = crate::service::build_command(entry.aliases[0], doc);
    let mut commands = Vec::new();
    walk(&cmd, doc, &mut Vec::new(), &mut commands);
    json!({
        "name": entry.aliases[0],
        "aliases": &entry.aliases[1..],
        "api": entry.api_name,
        "version": entry.version,
        "description": entry.description,
        "commands": commands,
    })
}

/// Inventory for an API that is not in the registry, named as
/// `<api>:<version>`: its name is the API name and its description comes
/// from the Discovery document.
fn unregistered_inventory(api: &str, version: &str, doc: &RestDescription) -> Value {
    let cmd = crate::service::build_command(api, doc);
    let mut commands = Vec::new();
    walk(&cmd, doc, &mut Vec::new(), &mut commands);
    json!({
        "name": api,
        "aliases": [],
        "api": api,
        "version": version,
        "description": doc.description.as_deref().unwrap_or_default(),
        "commands": commands,
    })
}

fn static_commands() -> Vec<Value> {
    // `auth` parses its own arguments: list its real subcommands and flags,
    // not the passthrough argument of the static tree.
    let root = crate::completions::with_auth_tree(crate::cli_args::command());
    let mut out = Vec::new();
    fn visit(cmd: &Command, path: &mut Vec<String>, out: &mut Vec<Value>) {
        for sub in cmd.get_subcommands() {
            path.push(sub.get_name().to_string());
            if sub.get_subcommands().next().is_none() {
                out.push(json!({
                    "path": path.join(" "),
                    "about": sub.get_about().map(|a| a.to_string()),
                    "flags": flags_of(sub),
                }));
            } else {
                visit(sub, path, out);
            }
            path.pop();
        }
    }
    visit(&root, &mut Vec::new(), &mut out);
    out
}

/// Resolve the services to list, each with the version to list; all
/// registered services at their default versions when `names` is empty.
///
/// A name is resolved like the service argument of a normal command: an
/// alias, `<alias>:<version>` or any Discovery API as `<api>:<version>`.
/// `api_version` (`--api-version`) wins over a `:version` suffix. It picks
/// one version, so it needs the services named: applied to every registered
/// service it would ask for a version most of them do not have. A service
/// named twice, or by two of its aliases (`reports admin-reports`), is
/// listed once.
fn select_services(
    names: &[String],
    api_version: Option<&str>,
) -> Result<Vec<ResolvedService>, GwsError> {
    if names.is_empty() {
        if api_version.is_some() {
            return Err(GwsError::Validation(
                "--api-version needs the services to list, e.g. `gwsr commands drive --api-version v2`"
                    .to_string(),
            ));
        }
        return Ok(SERVICES
            .iter()
            .map(|e| ResolvedService {
                api_name: e.api_name.to_string(),
                version: e.version.to_string(),
                entry: Some(e),
            })
            .collect());
    }
    let mut selected: Vec<ResolvedService> = Vec::new();
    for n in names {
        let r = crate::services::resolve_service_spec(n, api_version)?;
        if !selected
            .iter()
            .any(|s| s.api_name == r.api_name && s.version == r.version)
        {
            selected.push(r);
        }
    }
    Ok(selected)
}

/// Build the full inventory (fetches Discovery documents as needed).
/// `api_version` (`--api-version`) lists that version of the named services.
pub async fn build(names: &[String], api_version: Option<&str>) -> Result<Value, GwsError> {
    let selected = select_services(names, api_version)?;
    let docs = futures_util::future::try_join_all(
        selected
            .iter()
            .map(|s| crate::service::load_document(&s.api_name, &s.version)),
    )
    .await?;
    let services: Vec<Value> = selected
        .iter()
        .zip(&docs)
        .map(|(s, doc)| match s.entry {
            Some(entry) => {
                let mut inventory = service_inventory(entry, doc);
                inventory["version"] = json!(s.version);
                inventory
            }
            None => unregistered_inventory(&s.api_name, &s.version, doc),
        })
        .collect();
    Ok(json!({
        "gwsr_version": env!("CARGO_PKG_VERSION"),
        "global_flags": crate::cli_args::global_args().iter().map(flag_json).collect::<Vec<_>>(),
        "commands": static_commands(),
        "services": services,
    }))
}

/// Flatten an inventory into rows for table/CSV display.
pub fn rows(inventory: &Value) -> Value {
    let mut rows = Vec::new();
    for svc in inventory["services"].as_array().into_iter().flatten() {
        let service = svc["name"].as_str().unwrap_or_default();
        for cmd in svc["commands"].as_array().into_iter().flatten() {
            rows.push(json!({
                "command": format!("gwsr {service} {}", cmd["path"].as_str().unwrap_or_default()),
                "kind": cmd["kind"],
                "about": cmd["about"],
            }));
        }
    }
    Value::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::RestMethod;

    fn doc() -> RestDescription {
        let mut files = RestResource::default();
        files.methods.insert(
            "list".into(),
            RestMethod {
                id: Some("drive.files.list".into()),
                http_method: "GET".into(),
                scopes: vec!["https://www.googleapis.com/auth/drive.readonly".into()],
                ..Default::default()
            },
        );
        let mut perms = RestResource::default();
        perms.methods.insert(
            "create".into(),
            RestMethod {
                id: Some("drive.permissions.create".into()),
                http_method: "POST".into(),
                request: Some(crate::discovery::SchemaRef::default()),
                ..Default::default()
            },
        );
        files.resources.insert("permissions".into(), perms);
        let mut doc = RestDescription {
            name: "drive".into(),
            version: "v3".into(),
            ..Default::default()
        };
        doc.resources.insert("files".into(), files);
        doc
    }

    fn drive_entry() -> &'static ServiceEntry {
        SERVICES.iter().find(|e| e.aliases[0] == "drive").unwrap()
    }

    #[test]
    fn methods_and_nested_resources_are_listed() {
        let inv = service_inventory(drive_entry(), &doc());
        let cmds = inv["commands"].as_array().unwrap();
        let list = cmds.iter().find(|c| c["path"] == "files list").unwrap();
        assert_eq!(list["kind"], "method");
        assert_eq!(list["id"], "drive.files.list");
        assert_eq!(list["http_method"], "GET");
        let flags: Vec<&str> = list["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(flags.contains(&"--params"), "{flags:?}");
        assert!(
            !flags.contains(&"--format"),
            "global flags are listed once: {flags:?}"
        );
        let create = cmds
            .iter()
            .find(|c| c["path"] == "files permissions create")
            .unwrap();
        assert_eq!(create["http_method"], "POST");
        let cflags: Vec<&str> = create["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(cflags.contains(&"--json"), "{cflags:?}");
    }

    #[test]
    fn helpers_are_listed_with_flags() {
        let inv = service_inventory(drive_entry(), &doc());
        let helper = inv["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "helper")
            .expect("drive has helper commands");
        assert!(helper["path"].as_str().unwrap().starts_with('+'));
        assert!(helper["flags"].is_array());
    }

    /// Helpers with their own subcommands (`gmail +filter create|list|delete`)
    /// list each runnable subcommand with its flags, not just the group.
    #[test]
    fn helper_subcommands_are_listed_with_their_flags() {
        let entry = SERVICES.iter().find(|e| e.aliases[0] == "gmail").unwrap();
        let doc = RestDescription {
            name: "gmail".into(),
            version: "v1".into(),
            ..Default::default()
        };
        let inv = service_inventory(entry, &doc);
        let cmds = inv["commands"].as_array().unwrap();
        let paths: Vec<&str> = cmds.iter().map(|c| c["path"].as_str().unwrap()).collect();
        for p in ["+filter create", "+filter list", "+filter delete"] {
            assert!(paths.contains(&p), "missing {p}: {paths:?}");
        }
        assert!(
            !paths.contains(&"+filter"),
            "a helper group is not itself runnable: {paths:?}"
        );
        let create = cmds.iter().find(|c| c["path"] == "+filter create").unwrap();
        assert_eq!(create["kind"], "helper");
        let flags: Vec<&str> = create["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(flags.contains(&"--from"), "{flags:?}");
        assert!(flags.contains(&"--yes"), "{flags:?}");
    }

    #[test]
    fn static_commands_include_top_level_tools() {
        let paths: Vec<String> = static_commands()
            .iter()
            .map(|c| c["path"].as_str().unwrap().to_string())
            .collect();
        for p in [
            "schema",
            "commands",
            "completions",
            "cache clear",
            "dev generate-skills",
            "dev man",
        ] {
            assert!(paths.contains(&p.to_string()), "missing {p}: {paths:?}");
        }
    }

    fn selected(names: &[&str], api_version: Option<&str>) -> Vec<(String, String, bool)> {
        let names: Vec<String> = names.iter().map(|s| (*s).to_string()).collect();
        select_services(&names, api_version)
            .unwrap()
            .into_iter()
            .map(|s| (s.api_name, s.version, s.entry.is_some()))
            .collect()
    }

    /// `auth` parses its own arguments, so the static tree only has a
    /// passthrough for it; the inventory must list the real `auth`
    /// subcommands and their flags instead.
    #[test]
    fn static_commands_list_auth_subcommands_with_their_flags() {
        let commands = static_commands();
        let paths: Vec<&str> = commands
            .iter()
            .map(|c| c["path"].as_str().unwrap())
            .collect();
        for p in [
            "auth login",
            "auth status",
            "auth logout",
            "auth export",
            "auth setup",
        ] {
            assert!(paths.contains(&p), "missing {p}: {paths:?}");
        }
        assert!(
            !paths.contains(&"auth"),
            "auth is a group, not a command taking raw args: {paths:?}"
        );
        let login = commands.iter().find(|c| c["path"] == "auth login").unwrap();
        let flags: Vec<&str> = login["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(flags.contains(&"--scopes"), "{flags:?}");
        let setup = commands.iter().find(|c| c["path"] == "auth setup").unwrap();
        assert!(
            setup["flags"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["name"] != "args"),
            "auth setup lists its real flags: {setup}"
        );
    }

    #[test]
    fn unknown_service_is_rejected() {
        let err = select_services(&["nope".into()], None).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(err.to_string().contains("<api>:<version>"), "{err}");
        assert_eq!(selected(&["drive"], None).len(), 1);
        assert_eq!(selected(&[], None).len(), SERVICES.len());
    }

    /// `<api>:<version>` is accepted as it is by `schema`, `batch` and
    /// normal commands, for registered and unregistered APIs alike.
    #[test]
    fn api_version_specs_are_accepted() {
        assert_eq!(
            selected(&["youtube:v3", "drive:v2", "drive"], None),
            [
                ("youtube".into(), "v3".into(), false),
                ("drive".into(), "v2".into(), true),
                ("drive".into(), "v3".into(), true),
            ]
        );
        // The spec is validated like everywhere else.
        assert!(select_services(&["youtube:".into()], None).is_err());
        assert!(select_services(&["youtube:../v3".into()], None).is_err());
        assert!(select_services(&["../x:v1".into()], None).is_err());
    }

    #[test]
    fn unregistered_api_is_listed_under_its_api_name() {
        let inv = unregistered_inventory("drive", "v3", &doc());
        assert_eq!(inv["name"], "drive");
        assert_eq!(inv["aliases"], json!([]));
        assert_eq!(inv["version"], "v3");
        assert!(!inv["commands"].as_array().unwrap().is_empty(), "{inv}");
    }

    /// Naming a service twice, or by two aliases, listed (and fetched) it
    /// twice, so `services` held duplicate entries.
    #[test]
    fn a_service_named_twice_is_listed_once() {
        let listed: Vec<(String, String)> = selected(
            &["drive", "reports", "drive", "admin-reports", "gmail"],
            None,
        )
        .into_iter()
        .map(|(api, version, _)| (api, version))
        .collect();
        assert_eq!(
            listed,
            [
                ("drive".into(), "v3".into()),
                ("admin".into(), "reports_v1".into()),
                ("gmail".into(), "v1".into()),
            ]
        );
    }

    #[test]
    fn api_version_applies_to_the_named_services() {
        assert_eq!(
            selected(&["drive"], Some("v2")),
            [("drive".into(), "v2".into(), true)]
        );
        assert_eq!(
            selected(&["drive"], None),
            [("drive".into(), "v3".into(), true)]
        );
        // --api-version wins over a `:version` suffix.
        assert_eq!(
            selected(&["drive:v3"], Some("v2")),
            [("drive".into(), "v2".into(), true)]
        );
        // Without named services the flag would apply to every API.
        let err = select_services(&[], Some("v2")).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        // The version is validated like everywhere else.
        assert!(select_services(&["drive".into()], Some("../v2")).is_err());
    }

    #[test]
    fn rows_flatten_inventory() {
        let inv = json!({"services": [service_inventory(drive_entry(), &doc())]});
        let rows = rows(&inv);
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .any(|r| r["command"] == "gwsr drive files list")
        );
    }
}
