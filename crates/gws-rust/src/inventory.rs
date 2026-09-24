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
use crate::services::{SERVICES, ServiceEntry};

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
            out.push(json!({
                "path": path.join(" "),
                "kind": "helper",
                "about": sub.get_about().map(|a| a.to_string()),
                "flags": flags_of(sub),
            }));
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

/// Inventory for one service.
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

fn static_commands() -> Vec<Value> {
    let root = crate::cli_args::command();
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

/// Select service entries by alias; all when `names` is empty.
fn select_services(names: &[String]) -> Result<Vec<&'static ServiceEntry>, GwsError> {
    if names.is_empty() {
        return Ok(SERVICES.iter().collect());
    }
    names
        .iter()
        .map(|n| {
            SERVICES
                .iter()
                .find(|e| e.aliases.contains(&n.as_str()))
                .ok_or_else(|| GwsError::Validation(format!("unknown service '{n}'")))
        })
        .collect()
}

/// Build the full inventory (fetches Discovery documents as needed).
pub async fn build(names: &[String]) -> Result<Value, GwsError> {
    let entries = select_services(names)?;
    let docs = futures_util::future::try_join_all(
        entries
            .iter()
            .map(|e| crate::service::load_document(e.api_name, e.version)),
    )
    .await?;
    let services: Vec<Value> = entries
        .iter()
        .zip(&docs)
        .map(|(entry, doc)| service_inventory(entry, doc))
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

/// Aligned `command  description` lines for terminal display.
pub fn text_listing(inventory: &Value) -> String {
    let rows = rows(inventory);
    let rows = rows.as_array().map(Vec::as_slice).unwrap_or_default();
    let width = rows
        .iter()
        .filter_map(|r| r["command"].as_str())
        .map(|c| c.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let command = row["command"].as_str().unwrap_or_default();
        let about = row["about"].as_str().unwrap_or_default();
        let about = about.lines().next().unwrap_or_default();
        let line = format!(
            "{command:<width$}  {}",
            crate::output::sanitize_for_terminal(about)
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.trim_end().to_string()
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

    #[test]
    fn unknown_service_is_rejected() {
        assert!(select_services(&["nope".into()]).is_err());
        assert_eq!(select_services(&["drive".into()]).unwrap().len(), 1);
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
        let text = text_listing(&inv);
        assert!(
            text.lines().any(|l| l.starts_with("gwsr drive files list")),
            "{text}"
        );
    }
}
