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

//! Validates every `gwsr ...` command in `registry/recipes.toml` and
//! `registry/personas.toml` against the real clap command tree (UX P1-13).
//!
//! * Helper commands (`+send`, `+read`, ...) are parsed with the helpers' own
//!   clap definitions, including the root's global flags.
//! * Discovery commands (`gmail users messages list`) are parsed with
//!   `commands::build_cli` over stripped Discovery fixtures in
//!   `tests/fixtures/discovery/`, and `--params` keys are checked against the
//!   method's parameters. `--params`/`--json` must be valid JSON.
//! * A bare mention (`gwsr calendar +insert` with no flags) only needs the
//!   command to exist; any command with flags must parse completely.
//! * `auth` commands are checked against a curated subcommand list.

use crate::discovery::{RestDescription, RestMethod};
use serde_json::Value;

const RECIPES: &str = include_str!("../../registry/recipes.toml");
const PERSONAS: &str = include_str!("../../registry/personas.toml");
const AUTH_SUBCOMMANDS: &[&str] = &["login", "setup", "status", "export", "logout"];

/// Every string value anywhere in a TOML document.
fn strings(v: &toml::Value, out: &mut Vec<String>) {
    match v {
        toml::Value::String(s) => out.push(s.clone()),
        toml::Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        toml::Value::Table(t) => t.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// Every backtick-quoted span that starts with `gwsr `.
fn commands_in(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| span.starts_with("gwsr "))
        .map(str::to_string)
        .collect()
}

fn all_commands() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (file, src) in [("recipes.toml", RECIPES), ("personas.toml", PERSONAS)] {
        let doc: toml::Value =
            toml::from_str(src).unwrap_or_else(|e| panic!("{file} is not valid TOML: {e}"));
        let mut texts = Vec::new();
        strings(&doc, &mut texts);
        for t in texts {
            for c in commands_in(&t) {
                out.push((file.to_string(), c));
            }
        }
    }
    out
}

fn load_doc(service: &str) -> Result<RestDescription, String> {
    let (api, version) = crate::services::resolve_service(service)
        .map_err(|e| format!("unknown service '{service}': {e}"))?;
    let path = format!(
        "{}/tests/fixtures/discovery/{api}_{version}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("bad fixture {path}: {e}")),
        Err(_) => match crate::helpers::get_helper(&api) {
            Some(h) if h.helper_only() => Ok(RestDescription {
                name: api,
                ..Default::default()
            }),
            _ => Err(format!(
                "no Discovery fixture {path}; add one to validate '{service}' commands"
            )),
        },
    }
}

/// Find the Discovery method for a subcommand path like ["users", "messages", "list"].
fn find_method<'a>(doc: &'a RestDescription, path: &[&str]) -> Option<&'a RestMethod> {
    let (method, resources) = path.split_last()?;
    let (first, rest) = resources.split_first()?;
    let mut res = doc.resources.get(*first)?;
    for r in rest {
        res = res.resources.get(*r)?;
    }
    res.methods.get(*method)
}

fn validate(command: &str) -> Result<(), String> {
    let tokens = shell_words::split(command).map_err(|e| format!("shell quoting error: {e}"))?;
    let service = tokens.get(1).ok_or("missing service")?;
    if service == "auth" {
        let sub = tokens.get(2).ok_or("missing auth subcommand")?;
        return if AUTH_SUBCOMMANDS.contains(&sub.as_str()) {
            Ok(())
        } else {
            Err(format!("unknown auth subcommand '{sub}'"))
        };
    }
    let doc = load_doc(service)?;
    let cli = crate::commands::build_cli(&doc);
    let mut argv = vec!["gwsr".to_string()];
    argv.extend(tokens[2..].iter().cloned());
    let bare_mention = tokens[2..].iter().all(|t| !t.starts_with('-'));
    let matches = match cli.try_get_matches_from(&argv) {
        Ok(m) => m,
        Err(e) if bare_mention && e.kind() == clap::error::ErrorKind::MissingRequiredArgument => {
            return Ok(());
        }
        Err(e) => return Err(e.to_string().lines().next().unwrap_or_default().to_string()),
    };

    // Walk to the leaf subcommand.
    let mut path = Vec::new();
    let mut leaf = &matches;
    while let Some((name, sub)) = leaf.subcommand() {
        path.push(name);
        leaf = sub;
    }
    for flag in ["params", "json"] {
        if let Ok(Some(raw)) = leaf.try_get_one::<String>(flag) {
            let v: Value = serde_json::from_str(raw)
                .map_err(|e| format!("--{flag} is not valid JSON: {e}"))?;
            if flag == "params" && !path.first().is_some_and(|p| p.starts_with('+')) {
                let method = find_method(&doc, &path)
                    .ok_or_else(|| format!("no Discovery method for {path:?}"))?;
                let obj = v.as_object().ok_or("--params must be a JSON object")?;
                for key in obj.keys() {
                    if !method.parameters.contains_key(key) && !doc.parameters.contains_key(key) {
                        return Err(format!(
                            "--params key '{key}' is not a parameter of {}",
                            path.join(".")
                        ));
                    }
                }
                for (name, p) in &method.parameters {
                    if p.required
                        && p.location.as_deref() == Some("path")
                        && !obj.contains_key(name)
                    {
                        return Err(format!(
                            "--params is missing required path parameter '{name}'"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn registry_files_contain_commands() {
    let cmds = all_commands();
    assert!(
        cmds.len() > 50,
        "expected many commands, found {}",
        cmds.len()
    );
}

#[test]
fn every_registry_command_matches_the_real_command_tree() {
    let failures: Vec<String> = all_commands()
        .into_iter()
        .filter_map(|(file, cmd)| {
            validate(&cmd)
                .err()
                .map(|e| format!("{file}: `{cmd}`\n    -> {e}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} invalid registry command(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn validator_rejects_known_bad_commands() {
    for bad in [
        "gwsr docs +write --title T --body B",
        "gwsr drive +upload --file ./x.pdf",
        "gwsr events +renew --subscription s",
        "gwsr forms forms list",
        "gwsr gmail +read --id 123",
        "gwsr gmail users messages list --params '{\"q\": \"x\"}' --params-typo 1",
        "gwsr gmail users messages get --params '{\"userId\": \"me\", \"idd\": \"x\"}'",
        "gwsr gmail users messages get --params '{\"userId\": \"me\"}'",
        "gwsr gmail users messages modify --params '{\"userId\": \"me\", \"id\": \"x\"}' --json '{bad'",
        "gwsr nosuchservice things list",
        "gwsr auth nope",
    ] {
        assert!(validate(bad).is_err(), "should reject: {bad}");
    }
    for good in [
        "gwsr gmail +send",
        "gwsr gmail +triage --max 5",
        "gwsr gmail users messages get --params '{\"userId\": \"me\", \"id\": \"x\", \"format\": \"full\"}'",
        "gwsr drive files list --params '{\"q\": \"x\", \"fields\": \"files(id)\"}' --format table",
        "gwsr auth status",
    ] {
        assert!(
            validate(good).is_ok(),
            "should accept: {good}: {:?}",
            validate(good)
        );
    }
}
