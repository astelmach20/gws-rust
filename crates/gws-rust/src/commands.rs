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

use std::collections::{HashMap, HashSet};

use clap::{Arg, ArgAction, Command};

use crate::discovery::{RestDescription, RestMethod, RestResource};

/// Builds the full CLI command tree from a Discovery Document.
pub fn build_cli(doc: &RestDescription) -> Command {
    let about_text = doc
        .description
        .clone()
        .unwrap_or_else(|| "Google Workspace CLI".to_string());
    let mut root = Command::new("gwsr")
        .about(about_text)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .args(crate::cli_args::global_args());

    // `gwsr <service>` so usage lines read `gwsr drive files list`.
    root = root.bin_name(format!("gwsr {}", doc.name));

    // Inject helper commands
    let helper = crate::helpers::get_helper(&doc.name);
    if let Some(ref helper) = helper {
        root = helper.inject_commands(root, doc);
    }

    // Add resource subcommands (unless helper suppresses them)
    let skip_resources = helper.as_ref().is_some_and(|h| h.helper_only());
    if !skip_resources {
        // Methods declared outside any resource (e.g. `oauth2:v2 tokeninfo`),
        // then the resources.
        for (cli_name, node) in subcommand_names(&doc.methods, &doc.resources) {
            root = root.subcommand(build_node_command(doc, &cli_name, node));
        }
    }

    root
}

const HEADING_REQUEST: &str = "Request options";
const HEADING_OUTPUT: &str = "Output options";
const HEADING_PAGINATION: &str = "Pagination options";
const HEADING_UPLOAD: &str = "Upload options";

/// Whether a method is marked deprecated. Discovery marks some methods with
/// `deprecated: true` and many only in the description.
fn is_deprecated(method: &RestMethod) -> bool {
    method.deprecated
        || method.description.as_deref().is_some_and(|d| {
            d.trim_start()
                .to_ascii_lowercase()
                .starts_with("deprecated")
        })
}

/// Flags for one generated method command. Ids must match
/// `executor::options`.
pub fn method_args(doc: &RestDescription, method: &RestMethod) -> Vec<Arg> {
    let mut args = vec![
        Arg::new("params")
            .long("params")
            .help("URL/query parameters as a JSON object; @FILE reads a file, - reads stdin")
            .value_name("JSON")
            .help_heading(HEADING_REQUEST),
        Arg::new("fields")
            .long("fields")
            .help("Partial-response field mask, e.g. 'files(id,name)' (sets the `fields` parameter)")
            .value_name("MASK")
            .help_heading(HEADING_REQUEST),
        Arg::new("allow-unknown-params")
            .long("allow-unknown-params")
            .help("Send parameters that are not in the Discovery document instead of rejecting them")
            .action(ArgAction::SetTrue)
            .help_heading(HEADING_REQUEST),
        Arg::new("timeout")
            .long("timeout")
            .help("Seconds to wait for a response and between received bytes; 0 disables [env: GWSR_TIMEOUT] [default: 60]")
            .value_name("SECS")
            .help_heading(HEADING_REQUEST),
        Arg::new("no-quota-project")
            .long("no-quota-project")
            .help("Do not send the quota project (GWSR_PROJECT_ID, the OAuth client's project or ADC) as x-goog-user-project [env: GWSR_NO_QUOTA_PROJECT=1]")
            .action(ArgAction::SetTrue)
            .help_heading(HEADING_REQUEST),
        Arg::new("output")
            .long("output")
            .short('o')
            .help("Write the response payload to PATH (binary media, decoded base64 field, or the JSON) and print a JSON summary; - streams the raw payload to stdout")
            .value_name("PATH")
            .help_heading(HEADING_OUTPUT),
        Arg::new("decode-field")
            .long("decode-field")
            .help("Base64 field of a JSON response to decode into --output (e.g. data, raw, payload.body.data)")
            .value_name("FIELD")
            .requires("output")
            .help_heading(HEADING_OUTPUT),
    ];

    if method.request.is_some() {
        args.push(
            Arg::new("json")
                .long("json")
                .help("Request body as JSON; @FILE reads a file, - reads stdin")
                .value_name("JSON")
                .help_heading(HEADING_REQUEST),
        );
        args.push(
            Arg::new("allow-unknown-fields")
                .long("allow-unknown-fields")
                .help("Send body fields that are not in the Discovery schema (e.g. Developer Preview fields)")
                .action(ArgAction::SetTrue)
                .help_heading(HEADING_REQUEST),
        );
    }

    if method.supports_media_upload {
        args.push(
            Arg::new("upload")
                .long("upload")
                .help(
                    "Local file to upload as media content (resumable above 5 MiB when supported)",
                )
                .value_name("PATH")
                .help_heading(HEADING_UPLOAD),
        );
        args.push(
            Arg::new("upload-content-type")
                .long("upload-content-type")
                .help("MIME type of the uploaded content (default: from the file extension or metadata mimeType)")
                .value_name("MIME")
                .requires("upload")
                .help_heading(HEADING_UPLOAD),
        );
        if method.supports_resumable_upload() {
            args.push(
                Arg::new("upload-resumable")
                    .long("upload-resumable")
                    .help("Always use the resumable protocol (8 MiB chunks, resumes after network errors)")
                    .action(ArgAction::SetTrue)
                    .requires("upload")
                    .help_heading(HEADING_UPLOAD),
            );
        }
    }

    if method.pagination(doc).is_some() {
        args.push(
            Arg::new("page-all")
                .long("page-all")
                .help("Fetch every page, one JSON line per page (NDJSON)")
                .action(ArgAction::SetTrue)
                .help_heading(HEADING_PAGINATION),
        );
        args.push(
            Arg::new("page-items")
                .long("page-items")
                .help("Like --page-all but one JSON line per item; optionally name the list field (--page-items=files)")
                .value_name("FIELD")
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value("")
                .help_heading(HEADING_PAGINATION),
        );
        args.push(
            Arg::new("page-limit")
                .long("page-limit")
                .help("Stop after N pages (0 = unlimited, the default); a truncated run warns on stderr")
                .value_name("N")
                .value_parser(clap::value_parser!(u32))
                .help_heading(HEADING_PAGINATION),
        );
        args.push(
            Arg::new("page-delay")
                .long("page-delay")
                .help("Delay in milliseconds between page fetches (default: 100)")
                .value_name("MS")
                .value_parser(clap::value_parser!(u64))
                .help_heading(HEADING_PAGINATION),
        );
    }

    if crate::executor::operation::can_wait(doc, method) {
        args.push(
            Arg::new("wait")
                .long("wait")
                .help("Poll the returned long-running operation until it finishes and print its result")
                .action(ArgAction::SetTrue)
                .help_heading(HEADING_REQUEST),
        );
        args.push(
            Arg::new("wait-timeout")
                .long("wait-timeout")
                .help("Give up waiting after SECS (default: 600)")
                .value_name("SECS")
                .value_parser(clap::value_parser!(u64))
                .requires("wait")
                .help_heading(HEADING_REQUEST),
        );
    }

    if crate::executor::is_gated(method) {
        args.push(crate::confirm::yes_arg().help_heading(HEADING_REQUEST));
    }

    args
}

/// First sentence of a description, trimmed for a one-line table.
fn summary(desc: Option<&str>) -> String {
    let text = crate::text::truncate_description(desc.unwrap_or(""), 400, true);
    let first = text
        .split(". ")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.');
    let mut out: String = first.chars().take(90).collect();
    if first.chars().count() > 90 {
        out.push('…');
    }
    out
}

/// The "Parameters" block appended to a method's help.
pub fn parameters_help(doc: &RestDescription, method: &RestMethod) -> Option<String> {
    let mut params: Vec<(&String, &crate::discovery::MethodParameter)> = method
        .parameters
        .iter()
        .filter(|(_, p)| !p.deprecated)
        .collect();
    if params.is_empty() {
        return None;
    }
    params.sort_by(|(an, ap), (bn, bp)| bp.required.cmp(&ap.required).then(an.cmp(bn)));
    let width = params.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let mut text = String::from("Parameters (pass with --params '{\"name\": value}'):\n");
    for (name, p) in params {
        let mut ty = p.param_type.clone().unwrap_or_else(|| "string".into());
        if p.repeated {
            ty = format!("[{ty}]");
        }
        let flags = match (p.required, p.location.as_deref()) {
            (true, Some("path")) => "required, path",
            (true, _) => "required",
            (false, Some("path")) => "path",
            _ => "",
        };
        let mut line = format!("  {name:<width$}  {ty:<9}");
        if !flags.is_empty() {
            line.push_str(&format!(" ({flags})"));
        }
        let desc = summary(p.description.as_deref());
        if !desc.is_empty() {
            line.push_str("  ");
            line.push_str(&desc);
        }
        if let Some(values) = &p.enum_values {
            line.push_str(&format!(" [values: {}]", values.join(", ")));
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    if let Some(path) = crate::schema::method_schema_path(doc, method) {
        text.push_str(&format!(
            "\nFull request/response schema: gwsr schema {path}"
        ));
    }
    Some(text)
}

/// The command for one Discovery method (hidden when deprecated).
fn build_method_command(doc: &RestDescription, name: &str, method: &RestMethod) -> Command {
    let about = crate::text::truncate_description(
        method.description.as_deref().unwrap_or(""),
        crate::text::CLI_DESCRIPTION_LIMIT,
        true,
    );
    let mut cmd = Command::new(name.to_string())
        .about(about)
        .hide(is_deprecated(method))
        .args(method_args(doc, method));
    if let Some(help) = parameters_help(doc, method) {
        cmd = cmd.after_help(help);
    }
    cmd
}

/// The command for a method or resource, under its command name.
fn build_node_command(doc: &RestDescription, cli_name: &str, node: Node<'_>) -> Command {
    match node {
        Node::Method(_, method) => build_method_command(doc, cli_name, method),
        Node::Resource(name, resource) => build_resource_command(doc, cli_name, name, resource),
    }
}

/// Recursively builds the command for a resource that has commands (see
/// [`subcommand_names`]), hidden when all its children are.
fn build_resource_command(
    doc: &RestDescription,
    cli_name: &str,
    name: &str,
    resource: &RestResource,
) -> Command {
    let mut cmd = Command::new(cli_name.to_string())
        .about(format!("Operations on the '{name}' resource"))
        .subcommand_required(true)
        .arg_required_else_help(true);

    let mut all_hidden = true;
    for (child_name, node) in subcommand_names(&resource.methods, &resource.resources) {
        let child = build_node_command(doc, &child_name, node);
        all_hidden &= child.is_hide_set();
        cmd = cmd.subcommand(child);
    }
    cmd.hide(all_hidden)
}

/// A Discovery method or resource behind a generated subcommand, with its
/// Discovery name.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Method(&'a str, &'a RestMethod),
    Resource(&'a str, &'a RestResource),
}

/// Subcommand names clap claims in every command that has subcommands.
const RESERVED_NAMES: &[&str] = &["help"];

/// Whether a resource produces a command: it has a method, itself or in a
/// sub-resource.
fn has_commands(resource: &RestResource) -> bool {
    !resource.methods.is_empty() || resource.resources.values().any(has_commands)
}

/// The subcommands of one level of the Discovery tree, each with its command
/// name: the methods (sorted), then the resources that have commands
/// (sorted).
///
/// This is the one mapping between command names and Discovery names; the
/// builder, the request path, `gwsr schema`, `gwsr batch`, the inventory and
/// the skills generator all go through it. A command name is the Discovery
/// name, except where two subcommands would share a name (clap allows no
/// duplicates):
/// - a method and a resource with the same name: the method keeps the name
///   and the resource becomes `<name>-resource`;
/// - a name clap reserves (`help`): `help-method` or `help-resource`.
///
/// If that name is taken as well, `-2`, `-3`, ... is appended.
pub fn subcommand_names<'a>(
    methods: &'a HashMap<String, RestMethod>,
    resources: &'a HashMap<String, RestResource>,
) -> Vec<(String, Node<'a>)> {
    let mut nodes: Vec<Node<'a>> = methods
        .iter()
        .map(|(name, method)| Node::Method(name, method))
        .collect();
    nodes.sort_by_key(|node| node.name());
    let mut resource_nodes: Vec<Node<'a>> = resources
        .iter()
        .filter(|(_, resource)| has_commands(resource))
        .map(|(name, resource)| Node::Resource(name, resource))
        .collect();
    resource_nodes.sort_by_key(|node| node.name());
    nodes.extend(resource_nodes);

    // Every free Discovery name is kept; methods come first, so a method
    // wins over a resource of the same name.
    let mut taken: HashSet<String> = HashSet::new();
    let mut names: Vec<Option<String>> = nodes
        .iter()
        .map(|node| {
            let name = node.name();
            (!RESERVED_NAMES.contains(&name) && taken.insert(name.to_string()))
                .then(|| name.to_string())
        })
        .collect();
    // The others get their kind appended.
    for (node, slot) in nodes.iter().zip(names.iter_mut()) {
        if slot.is_some() {
            continue;
        }
        let kind = match node {
            Node::Method(..) => "method",
            Node::Resource(..) => "resource",
        };
        let base = format!("{}-{kind}", node.name());
        let mut candidate = base.clone();
        let mut n = 2;
        while !taken.insert(candidate.clone()) {
            candidate = format!("{base}-{n}");
            n += 1;
        }
        *slot = Some(candidate);
    }

    names
        .into_iter()
        .zip(nodes)
        .filter_map(|(name, node)| name.map(|name| (name, node)))
        .collect()
}

impl<'a> Node<'a> {
    /// The Discovery name.
    pub fn name(&self) -> &'a str {
        match self {
            Node::Method(name, _) | Node::Resource(name, _) => name,
        }
    }
}

/// The method or resource that a path of command names (`["files", "list"]`)
/// leads to in `doc`, following [`subcommand_names`]. `None` when a name is
/// unknown or follows a method.
pub fn resolve_command_path<'a>(doc: &'a RestDescription, path: &[&str]) -> Option<Node<'a>> {
    let (mut methods, mut resources) = (&doc.methods, &doc.resources);
    let mut found = None;
    for name in path {
        if matches!(found, Some(Node::Method(..))) {
            return None;
        }
        let (_, node) = subcommand_names(methods, resources)
            .into_iter()
            .find(|(cli_name, _)| cli_name == name)?;
        if let Node::Resource(_, resource) = node {
            (methods, resources) = (&resource.methods, &resource.resources);
        }
        found = Some(node);
    }
    found
}

/// The method that a path of command names leads to, if it is a method.
pub fn command_method<'a>(doc: &'a RestDescription, path: &[&str]) -> Option<&'a RestMethod> {
    match resolve_command_path(doc, path)? {
        Node::Method(_, method) => Some(method),
        Node::Resource(..) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "list".to_string(),
            RestMethod {
                id: None,
                description: None,
                http_method: "GET".to_string(),
                path: "list".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
                flat_path: None,
                supports_media_download: false,
                use_media_download_service: false,
                supports_media_upload: false,
                media_upload: None,
                deprecated: false,
            },
        );

        methods.insert(
            "delete".to_string(),
            RestMethod {
                id: None,
                description: None,
                http_method: "DELETE".to_string(),
                path: "delete".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/drive".to_string()],
                flat_path: None,
                supports_media_download: false,
                use_media_download_service: false,
                supports_media_upload: false,
                media_upload: None,
                deprecated: false,
            },
        );

        let mut resources = HashMap::new();
        resources.insert(
            "files".to_string(),
            RestResource {
                methods,
                resources: HashMap::new(),
            },
        );

        RestDescription {
            name: "drive".to_string(),
            version: "v3".to_string(),
            title: None,
            description: None,
            root_url: "".to_string(),
            mtls_root_url: None,
            service_path: "".to_string(),
            base_url: None,
            batch_path: None,
            schemas: HashMap::new(),
            resources,
            methods: HashMap::new(),
            parameters: HashMap::new(),
            auth: None,
        }
    }

    #[test]
    fn top_level_methods_become_root_commands() {
        let mut doc = make_doc();
        doc.methods.insert(
            "about".into(),
            RestMethod {
                id: Some("drive.about".into()),
                description: Some("Service info.".into()),
                http_method: "GET".into(),
                ..Default::default()
            },
        );
        let cmd = build_cli(&doc);
        let about = cmd.find_subcommand("about").expect("top-level method");
        assert!(about.get_arguments().any(|a| a.get_id() == "params"));
        assert!(cmd.find_subcommand("files").is_some());
    }

    #[test]
    fn test_all_commands_always_shown() {
        let doc = make_doc();
        let cmd = build_cli(&doc);

        // Should have "files" subcommand
        let files_cmd = cmd
            .find_subcommand("files")
            .expect("files resource missing");

        // All methods should always be visible regardless of auth state
        assert!(files_cmd.find_subcommand("list").is_some());
        assert!(files_cmd.find_subcommand("delete").is_some());
    }

    #[test]
    fn test_sanitize_arg_present() {
        let doc = make_doc();
        let cmd = build_cli(&doc);

        // The --sanitize global arg should be available
        let args: Vec<_> = cmd.get_arguments().collect();
        let sanitize_arg = args.iter().find(|a| a.get_id() == "sanitize");
        assert!(
            sanitize_arg.is_some(),
            "--sanitize arg should be present on root command"
        );
    }

    fn method_names(cmd: &Command) -> Vec<String> {
        cmd.get_arguments()
            .map(|a| a.get_id().to_string())
            .collect()
    }

    #[test]
    fn test_usage_includes_service_name() {
        let mut cmd = build_cli(&make_doc());
        cmd.build();
        let files = cmd.find_subcommand_mut("files").unwrap();
        let list = files.find_subcommand_mut("list").unwrap();
        let usage = list.render_usage().to_string();
        assert!(usage.contains("gwsr drive files list"), "{usage}");
    }

    #[test]
    fn test_pagination_flags_only_on_paginating_methods() {
        let doc = make_doc();
        let mut paged = RestMethod {
            http_method: "GET".to_string(),
            ..Default::default()
        };
        paged.parameters.insert(
            "pageToken".to_string(),
            crate::discovery::MethodParameter {
                location: Some("query".to_string()),
                ..Default::default()
            },
        );
        let ids: Vec<String> = method_args(&doc, &paged)
            .iter()
            .map(|a| a.get_id().to_string())
            .collect();
        assert!(ids.contains(&"page-all".to_string()));
        assert!(ids.contains(&"page-items".to_string()));

        let cmd = build_cli(&doc);
        let delete = cmd
            .find_subcommand("files")
            .unwrap()
            .find_subcommand("delete")
            .unwrap();
        let names = method_names(delete);
        assert!(!names.contains(&"page-all".to_string()));
        assert!(names.contains(&"yes".to_string()), "DELETE gets --yes");
        assert!(names.contains(&"fields".to_string()));
        let list = cmd
            .find_subcommand("files")
            .unwrap()
            .find_subcommand("list")
            .unwrap();
        assert!(!method_names(list).contains(&"yes".to_string()));
    }

    #[test]
    fn test_method_help_lists_parameters_and_hides_deprecated() {
        let mut p = std::collections::HashMap::new();
        p.insert(
            "fileId".to_string(),
            crate::discovery::MethodParameter {
                param_type: Some("string".into()),
                location: Some("path".into()),
                required: true,
                description: Some("The ID of the file. More text.".into()),
                ..Default::default()
            },
        );
        p.insert(
            "oldFlag".to_string(),
            crate::discovery::MethodParameter {
                deprecated: true,
                ..Default::default()
            },
        );
        let mut doc = make_doc();
        doc.resources.get_mut("files").unwrap().methods.insert(
            "get".into(),
            RestMethod {
                id: Some("drive.files.get".into()),
                parameters: p,
                ..Default::default()
            },
        );
        let m = &doc.resources["files"].methods["get"];
        let help = parameters_help(&doc, m).unwrap();
        assert!(help.contains("fileId"));
        assert!(help.contains("(required, path)"));
        assert!(help.contains("The ID of the file"));
        assert!(!help.contains("More text"));
        assert!(!help.contains("oldFlag"));
        assert!(help.contains("gwsr schema drive.files.get"));
    }

    #[test]
    fn test_methods_flagged_deprecated_in_discovery_are_hidden() {
        // Shape of admin directory_v1 `chromeosdevices`: `action` carries
        // `deprecated: true` but its description doesn't say so.
        let doc: RestDescription = serde_json::from_value(serde_json::json!({
            "name": "admin",
            "version": "directory_v1",
            "rootUrl": "https://admin.googleapis.com/",
            "servicePath": "",
            "resources": {"chromeosdevices": {"methods": {
                "action": {
                    "httpMethod": "POST",
                    "path": "admin/directory/v1/customer/{customerId}/devices/chromeos/{resourceId}/action",
                    "description": "Use BatchChangeChromeOsDeviceStatus instead.",
                    "deprecated": true
                },
                "get": {
                    "httpMethod": "GET",
                    "path": "admin/directory/v1/customer/{customerId}/devices/chromeos/{deviceId}",
                    "description": "Retrieves a Chrome OS device's properties."
                }
            }}}
        }))
        .unwrap();
        let cmd = build_cli(&doc);
        let devices = cmd.find_subcommand("chromeosdevices").unwrap();
        assert!(devices.find_subcommand("action").unwrap().is_hide_set());
        assert!(!devices.find_subcommand("get").unwrap().is_hide_set());
        assert!(!devices.is_hide_set());
    }

    #[test]
    fn test_deprecated_methods_and_resources_are_hidden() {
        let mut doc = make_doc();
        let mut team = RestResource::default();
        team.methods.insert(
            "list".into(),
            RestMethod {
                description: Some("Deprecated: Use drives.list instead.".into()),
                http_method: "GET".into(),
                ..Default::default()
            },
        );
        doc.resources.insert("teamdrives".into(), team);
        let cmd = build_cli(&doc);
        let team_cmd = cmd.find_subcommand("teamdrives").unwrap();
        assert!(team_cmd.is_hide_set());
        assert!(team_cmd.find_subcommand("list").unwrap().is_hide_set());
        assert!(!cmd.find_subcommand("files").unwrap().is_hide_set());
    }

    fn get(id: &str) -> RestMethod {
        RestMethod {
            id: Some(id.into()),
            http_method: "GET".into(),
            ..Default::default()
        }
    }

    fn resource(methods: &[(&str, &str)]) -> RestResource {
        let mut r = RestResource::default();
        for (name, id) in methods {
            r.methods.insert((*name).into(), get(id));
        }
        r
    }

    /// Every kind of name collision: a top-level method named like a
    /// resource, a method and a sub-resource of one name, and `help`.
    fn colliding_doc() -> RestDescription {
        let mut items = resource(&[("list", "x.items.list"), ("labels", "x.items.labels")]);
        items.resources.insert(
            "labels".into(),
            resource(&[("list", "x.items.labels.list")]),
        );
        items
            .resources
            .insert("help".into(), resource(&[("list", "x.items.help.list")]));
        let mut doc = RestDescription {
            name: "x".into(),
            ..Default::default()
        };
        doc.methods.insert("items".into(), get("x.items"));
        doc.methods.insert("help".into(), get("x.help"));
        doc.resources.insert("items".into(), items);
        doc
    }

    /// Colliding names never fail clap's own consistency checks (a panic in
    /// debug builds), and every method runs under its command name.
    #[test]
    fn colliding_names_get_distinct_commands() {
        let doc = colliding_doc();
        let cmd = build_cli(&doc);
        cmd.clone().debug_assert();
        let names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert_eq!(names, ["help-method", "items", "items-resource"]);
        let items = cmd.find_subcommand("items-resource").unwrap();
        let names: Vec<&str> = items.get_subcommands().map(|s| s.get_name()).collect();
        assert_eq!(
            names,
            ["labels", "list", "help-resource", "labels-resource"]
        );

        for (path, id) in [
            (&["items"][..], "x.items"),
            (&["help-method"], "x.help"),
            (&["items-resource", "list"], "x.items.list"),
            (&["items-resource", "labels"], "x.items.labels"),
            (
                &["items-resource", "labels-resource", "list"],
                "x.items.labels.list",
            ),
            (
                &["items-resource", "help-resource", "list"],
                "x.items.help.list",
            ),
        ] {
            let method = command_method(&doc, path).unwrap();
            assert_eq!(method.id.as_deref(), Some(id), "{path:?}");
            let mut argv = vec!["gwsr"];
            argv.extend_from_slice(path);
            build_cli(&doc).try_get_matches_from(argv).unwrap();
        }
        assert!(command_method(&doc, &["items-resource"]).is_none());
        assert!(command_method(&doc, &["items", "list"]).is_none());
        assert!(command_method(&doc, &["help"]).is_none());
    }

    /// A suffixed name that is itself taken gets a number; a resource
    /// without methods makes no command, so it renames nothing.
    #[test]
    fn suffixed_names_never_collide() {
        let mut methods = HashMap::new();
        methods.insert("a".to_string(), get("a"));
        methods.insert("a-resource".to_string(), get("a-resource"));
        methods.insert("b".to_string(), get("b"));
        let mut resources = HashMap::new();
        resources.insert("a".to_string(), resource(&[("list", "a.list")]));
        resources.insert("b".to_string(), RestResource::default());
        let names: Vec<String> = subcommand_names(&methods, &resources)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["a", "a-resource", "b", "a-resource-2"]);
    }
}
