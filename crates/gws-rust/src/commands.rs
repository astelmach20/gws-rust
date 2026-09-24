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
        .arg(
            clap::Arg::new("sanitize")
                .long("sanitize")
                .help("Sanitize API responses through a Model Armor template. Requires cloud-platform scope. Format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE. Also reads GWSR_SANITIZE_TEMPLATE env var.")
                .value_name("TEMPLATE")
                .global(true),
        )
        .arg(
            clap::Arg::new("dry-run")
                .long("dry-run")
                .help("Validate the request locally without sending it to the API")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            clap::Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        );

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
        let mut resource_names: Vec<_> = doc.resources.keys().collect();
        resource_names.sort();
        for name in resource_names {
            let resource = &doc.resources[name];
            if let Some(cmd) = build_resource_command(doc, name, resource) {
                root = root.subcommand(cmd);
            }
        }
    }

    root
}

const HEADING_REQUEST: &str = "Request options";
const HEADING_OUTPUT: &str = "Output options";
const HEADING_PAGINATION: &str = "Pagination options";
const HEADING_UPLOAD: &str = "Upload options";

/// Whether a method is marked deprecated. Discovery marks some methods with
/// `deprecated: true` (not modelled yet) and many only in the description.
fn is_deprecated(method: &RestMethod) -> bool {
    method.description.as_deref().is_some_and(|d| {
        d.trim_start()
            .to_ascii_lowercase()
            .starts_with("deprecated")
    })
}

/// The response schema looks like a long-running Operation.
fn returns_operation(method: &RestMethod) -> bool {
    method
        .response
        .as_ref()
        .and_then(|r| r.schema_ref.as_deref())
        .is_some_and(|r| r.ends_with("Operation"))
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
            .help("Do not send the ADC quota project as x-goog-user-project [env: GWSR_NO_QUOTA_PROJECT=1]")
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
        if crate::executor::supports_resumable_upload(method) {
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

    if crate::executor::paginates(doc, method) {
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

    if returns_operation(method) {
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

    if crate::executor::is_destructive(method) {
        args.push(
            Arg::new("yes")
                .long("yes")
                .short('y')
                .help("Confirm this destructive operation without prompting [env: GWSR_CONFIRM_DESTRUCTIVE]")
                .action(ArgAction::SetTrue)
                .help_heading(HEADING_REQUEST),
        );
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
pub fn parameters_help(method: &RestMethod) -> Option<String> {
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
    if let Some(id) = &method.id {
        text.push_str(&format!("\nFull request/response schema: gwsr schema {id}"));
    }
    Some(text)
}

/// Recursively builds a Command for a resource.
/// Returns None if the resource has no methods or sub-resources.
fn build_resource_command(
    doc: &RestDescription,
    name: &str,
    resource: &RestResource,
) -> Option<Command> {
    let mut cmd = Command::new(name.to_string())
        .about(format!("Operations on the '{name}' resource"))
        .subcommand_required(true)
        .arg_required_else_help(true);

    let mut has_children = false;
    let mut all_hidden = true;

    let mut method_names: Vec<_> = resource.methods.keys().collect();
    method_names.sort();
    for method_name in method_names {
        let method = &resource.methods[method_name];
        has_children = true;

        let about = crate::text::truncate_description(
            method.description.as_deref().unwrap_or(""),
            crate::text::CLI_DESCRIPTION_LIMIT,
            true,
        );
        let deprecated = is_deprecated(method);
        all_hidden &= deprecated;

        let mut method_cmd = Command::new(method_name.to_string())
            .about(about)
            .hide(deprecated)
            .args(method_args(doc, method));
        if let Some(help) = parameters_help(method) {
            method_cmd = method_cmd.after_help(help);
        }
        cmd = cmd.subcommand(method_cmd);
    }

    let mut sub_names: Vec<_> = resource.resources.keys().collect();
    sub_names.sort();
    for sub_name in sub_names {
        let sub_resource = &resource.resources[sub_name];
        if let Some(sub_cmd) = build_resource_command(doc, sub_name, sub_resource) {
            has_children = true;
            all_hidden &= sub_cmd.is_hide_set();
            cmd = cmd.subcommand(sub_cmd);
        }
    }

    if has_children {
        Some(cmd.hide(all_hidden))
    } else {
        None
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
                supports_media_upload: false,
                media_upload: None,
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
                supports_media_upload: false,
                media_upload: None,
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
            parameters: HashMap::new(),
            auth: None,
        }
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
        let m = RestMethod {
            id: Some("drive.files.get".into()),
            parameters: p,
            ..Default::default()
        };
        let help = parameters_help(&m).unwrap();
        assert!(help.contains("fileId"));
        assert!(help.contains("(required, path)"));
        assert!(help.contains("The ID of the file"));
        assert!(!help.contains("More text"));
        assert!(!help.contains("oldFlag"));
        assert!(help.contains("gwsr schema drive.files.get"));
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
}
