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

//! Runs every `gwsr` example in `README.md`, `CONTEXT.md` and `skills/*/SKILL.md`
//! through the real binary with `--dry-run`, so an example the CLI rejects
//! (a flag combination, a value format, a placeholder resource name) cannot be
//! documented.
//!
//! * Examples come from fenced code blocks (continuation lines and multi-line
//!   quoted arguments joined, the part of a pipeline starting at `gwsr`) and
//!   from inline `` `gwsr ...` `` spans in prose.
//! * An example must exit 0, or 2: it got past argument validation and
//!   needed credentials, which the test never has.
//! * A usage synopsis (it contains a placeholder such as `<ID>`) lists only
//!   clap-required flags, so its sample values ([`PLACEHOLDERS`]) may still
//!   fail the command's own checks. It must parse: no unknown flag,
//!   subcommand or enum value.
//! * A bare mention (`gwsr gmail +send` in prose, no flags) must name an
//!   existing command; it runs with `--help`.
//! * Input files the example names (`--file ./report.docx`, `--dir ./src`)
//!   are created in a fresh working directory, and stdin carries a small JSON
//!   object for `-` arguments. Examples reading `@file` arguments are skipped:
//!   their content is not in the docs.
//! * Discovery documents come from `tests/fixtures/discovery/` (missing
//!   request schemas are stubbed). Generated commands of services without a
//!   fixture are skipped, as are commands that do not need a Discovery
//!   document or change local state (`auth`, `schema`, ...).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Sample values for the upper-case usage placeholders clap prints.
const PLACEHOLDERS: &[(&str, &str)] = &[
    ("<APP>", "drive"),
    ("<DATE>", "2026-06-17"),
    ("<DIR>", "./out"),
    ("<EMAIL>", "ann@example.com"),
    ("<EMAILS>", "ann@example.com"),
    ("<FILE>", "input.txt"),
    ("<FORMAT>", "pdf"),
    ("<ID>", "abc123"),
    ("<JSON>", "{}"),
    ("<LOCATION>", "us-central1"),
    ("<NAME>", "Sample"),
    ("<PATH>", "input.txt"),
    ("<PROJECT>", "my-project"),
    ("<QUERY>", "is:unread"),
    ("<RANGE>", "Sheet1!A1"),
    ("<RESPONSE>", "accepted"),
    ("<ROLE>", "reader"),
    ("<SUBJECT>", "Hello"),
    ("<TEXT>", "sample text"),
    ("<TIME>", "2026-06-17T09:00"),
    ("<TITLE>", "Sample"),
    (
        "<URL>",
        "https://mail.google.com/mail/u/0/#inbox/18f1a2b3c4d",
    ),
    ("<VALUES>", "a,b"),
];

/// Top-level commands that are not services (and `auth`, which must never run
/// a real sign-in from a test).
const NOT_SERVICES: &[&str] = &[
    "auth",
    "batch",
    "cache",
    "commands",
    "completions",
    "dev",
    "help",
    "schema",
];

/// Services with helpers but no stripped fixture, seeded with an empty
/// Discovery document: `(cache api name, version, aliases)`.
const EMPTY_DOCS: &[(&str, &str, &[&str])] = &[
    (
        "admin",
        "directory_v1",
        &["admin", "directory", "admin-directory"],
    ),
    ("modelarmor", "v1", &["modelarmor"]),
];

/// Helper-only services, which need no Discovery document.
const HELPER_ONLY: &[&str] = &["workflow", "wf"];

/// Flags whose value is a local input file or directory the example expects.
const INPUT_FILE_FLAGS: &[&str] = &[
    "--file",
    "--text-file",
    "--csv-file",
    "--attach",
    "-a",
    "--upload",
    "--password-file",
];
const INPUT_DIR_FLAGS: &[&str] = &["--dir"];

/// clap's own parse errors: a synopsis must not produce these.
const CLAP_ERRORS: &[&str] = &[
    "unexpected argument",
    "invalid value",
    "unrecognized subcommand",
    "a value is required",
];

/// What stdin carries (for `--json -`, `--text-file -`, `--password-file -`).
const STDIN: &str = r#"{"title": "Sample stdin"}"#;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn doc_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = vec![root.join("README.md"), root.join("CONTEXT.md")];
    let mut skills: Vec<PathBuf> = std::fs::read_dir(root.join("skills"))
        .unwrap()
        .map(|e| e.unwrap().path().join("SKILL.md"))
        .filter(|p| p.is_file())
        .collect();
    skills.sort();
    files.extend(skills);
    files
}

/// `gwsr ...` commands in fenced blocks and inline code spans.
fn examples(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut pending = String::new();
    // Prose since the last fence: inline spans may wrap across lines.
    let mut prose = String::new();
    let spans = |prose: &mut String, out: &mut Vec<String>| {
        for span in prose.split('`').skip(1).step_by(2) {
            if span.starts_with("gwsr ") {
                out.push(span.to_string());
            }
        }
        prose.clear();
    };
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if !in_fence {
                spans(&mut prose, &mut out);
            }
            in_fence = !in_fence;
            pending.clear();
            continue;
        }
        if !in_fence {
            prose.push_str(line);
            prose.push('\n');
            continue;
        }
        if pending.is_empty() {
            let line = line.trim();
            let line = line.strip_prefix("$ ").unwrap_or(line);
            match gwsr_start(line) {
                Some(i) => pending.push_str(&line[i..]),
                None => continue,
            }
        } else {
            pending.push_str(line);
        }
        if let Some(head) = pending.strip_suffix('\\') {
            pending = format!("{} ", head.trim_end());
            continue;
        }
        // A quoted argument that continues on the next line.
        if shell_words::split(strip_comment(&pending)).is_err() {
            pending.push('\n');
            continue;
        }
        out.push(std::mem::take(&mut pending));
    }
    spans(&mut prose, &mut out);
    out
}

/// `line` without a trailing shell comment (an unquoted `#` after white space).
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '#') if prev.is_whitespace() => return &line[..i],
            _ => {}
        }
        prev = c;
    }
    line
}

/// Byte offset of a `gwsr` command at the start of the line or after a pipe.
fn gwsr_start(line: &str) -> Option<usize> {
    if line.starts_with("gwsr ") {
        return Some(0);
    }
    line.find("| gwsr ").map(|i| i + 2)
}

/// The command's own tokens: up to the first shell operator or comment.
fn command_tokens(example: &str) -> Result<Vec<String>, String> {
    let tokens =
        shell_words::split(strip_comment(example)).map_err(|e| format!("shell quoting: {e}"))?;
    Ok(tokens
        .into_iter()
        .take_while(|t| {
            !matches!(t.as_str(), "|" | "||" | "&&" | ";" | ">" | ">>" | "<")
                && !t.starts_with("2>")
        })
        .collect())
}

enum Kind {
    Example,
    Synopsis,
}

enum Plan {
    Run(Kind, Vec<String>),
    Skip,
}

fn plan(example: &str, fixtures: &Fixtures) -> Result<Plan, String> {
    // Shell variables, command substitution and elided values.
    if example.contains('$') || example.contains('…') || example.contains("...") {
        return Ok(Plan::Skip);
    }
    let tokens = command_tokens(example)?;
    let Some(service) = tokens.get(1) else {
        return Ok(Plan::Skip);
    };
    if service.starts_with('-') || NOT_SERVICES.contains(&service.as_str()) {
        return Ok(Plan::Skip);
    }
    let mut kind = Kind::Example;
    let mut args = Vec::with_capacity(tokens.len());
    for t in &tokens[1..] {
        if t.starts_with('[') || t.starts_with('@') {
            return Ok(Plan::Skip);
        }
        let mut t = t.clone();
        for (placeholder, value) in PLACEHOLDERS {
            if t.contains(placeholder) {
                t = t.replace(placeholder, value);
                kind = Kind::Synopsis;
            }
        }
        if let Some(open) = t.find('<')
            && t[open..].contains('>')
        {
            let rest = &t[open + 1..];
            if rest.starts_with(|c: char| c.is_ascii_lowercase()) {
                // `<resource> <method>` synopsis.
                return Ok(Plan::Skip);
            }
            if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                return Err(format!(
                    "unknown usage placeholder in {t:?}; add it to PLACEHOLDERS"
                ));
            }
        }
        args.push(t);
    }

    let helper = args.get(1).is_some_and(|a| a.starts_with('+'));
    let has_fixture = fixtures.services.iter().any(|f| f == service);
    let has_doc = has_fixture
        || HELPER_ONLY.contains(&service.as_str())
        || EMPTY_DOCS
            .iter()
            .any(|(_, _, aliases)| aliases.contains(&service.as_str()));
    if !has_doc {
        if helper {
            return Err(format!(
                "no Discovery fixture for '{service}'; add one to tests/fixtures/discovery/ or EMPTY_DOCS"
            ));
        }
        return Ok(Plan::Skip);
    }
    if !helper && !has_fixture && args.len() > 1 && !args[1].starts_with('-') {
        return Ok(Plan::Skip);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Plan::Run(Kind::Example, args));
    }
    if args.iter().skip(1).all(|a| !a.starts_with('-')) {
        // A bare mention: the command must exist.
        args.push("--help".to_string());
        return Ok(Plan::Run(Kind::Example, args));
    }
    if !helper {
        let path: Vec<&str> = args
            .iter()
            .take_while(|a| !a.starts_with('-'))
            .map(String::as_str)
            .collect();
        if fixtures.stubbed.contains(&path.join(" ")) {
            args.push("--allow-unknown-fields".to_string());
        }
    }
    if !args.iter().any(|a| a == "--dry-run") {
        args.push("--dry-run".to_string());
    }
    Ok(Plan::Run(kind, args))
}

/// Add an empty stub for every request schema a stripped fixture dropped.
///
/// Returns the command paths (`resource ... method`) whose body schema is a
/// stub: their `--json` examples run with `--allow-unknown-fields`, since the
/// fixture cannot say which fields are valid.
fn stub_request_schemas(doc: &mut Value) -> Vec<String> {
    fn refs(resources: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
        let Some(resources) = resources.as_object() else {
            return;
        };
        for (rname, r) in resources {
            let path = format!("{prefix}{rname} ");
            if let Some(methods) = r["methods"].as_object() {
                for (mname, m) in methods {
                    if let Some(schema) = m["request"]["$ref"].as_str() {
                        out.push((format!("{path}{mname}"), schema.to_string()));
                    }
                }
            }
            refs(&r["resources"], &path, out);
        }
    }
    let mut methods = Vec::new();
    refs(&doc["resources"], "", &mut methods);
    let obj = doc.as_object_mut().unwrap();
    let schemas = obj.entry("schemas").or_insert_with(|| json!({}));
    let schemas = schemas.as_object_mut().unwrap();
    let mut stubbed = Vec::new();
    for (path, name) in methods {
        if !schemas.contains_key(&name) {
            schemas.insert(name.clone(), json!({ "id": name, "type": "object" }));
            stubbed.push(path);
        } else if schemas[&name]["id"] == json!(name) && schemas[&name].get("properties").is_none()
        {
            stubbed.push(path);
        }
    }
    stubbed
}

/// The seeded Discovery cache.
struct Fixtures {
    /// Service names (as typed on the command line) with a fixture.
    services: Vec<String>,
    /// `service resource ... method` paths whose request schema is a stub.
    stubbed: Vec<String>,
}

/// Seed the Discovery cache from the stripped fixtures.
fn seed_cache(cache: &Path) -> Fixtures {
    let discovery = cache.join("discovery");
    std::fs::create_dir_all(&discovery).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discovery");
    let mut services = Vec::new();
    let mut stubbed = Vec::new();
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let stubs = stub_request_schemas(&mut doc);
        let name = doc["name"].as_str().unwrap().to_string();
        let version = doc["version"].as_str().unwrap().to_string();
        std::fs::write(
            discovery.join(format!("{name}+{version}.json")),
            serde_json::to_vec(&doc).unwrap(),
        )
        .unwrap();
        let service = match (name.as_str(), version.as_str()) {
            ("admin", "reports_v1") => "admin-reports".to_string(),
            ("workspaceevents", _) => "events".to_string(),
            _ => name,
        };
        stubbed.extend(stubs.into_iter().map(|p| format!("{service} {p}")));
        services.push(service);
    }
    for (api, version, _) in EMPTY_DOCS {
        let doc = json!({
            "name": api,
            "version": version,
            "rootUrl": format!("https://{api}.googleapis.com/"),
            "servicePath": "",
            "resources": {},
        });
        std::fs::write(
            discovery.join(format!("{api}+{version}.json")),
            serde_json::to_vec(&doc).unwrap(),
        )
        .unwrap();
    }
    Fixtures { services, stubbed }
}

/// Create the input files and directories an example names.
fn create_inputs(cwd: &Path, args: &[String]) {
    let write = |rel: &str| {
        let path = cwd.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content: &[u8] = match path.extension().and_then(|e| e.to_str()) {
            Some("csv") => b"a,b\n1,2\n",
            Some("json") => b"{}",
            Some("md") => b"# Title\n\nBody\n",
            _ => b"sample content\n",
        };
        std::fs::write(path, content).unwrap();
    };
    let script_project = |dir: &str| {
        write(&format!("{dir}/Code.gs"));
        std::fs::write(
            cwd.join(dir).join("appsscript.json"),
            br#"{"timeZone": "UTC"}"#,
        )
        .unwrap();
    };
    for pair in args.windows(2) {
        let (flag, value) = (pair[0].as_str(), pair[1].as_str());
        if value == "-" {
            continue;
        }
        if INPUT_FILE_FLAGS.contains(&flag) {
            write(value);
        } else if INPUT_DIR_FLAGS.contains(&flag) {
            script_project(value);
        }
    }
    // `script +push` without --dir pushes the working directory.
    if args.iter().any(|a| a == "+push") && !args.iter().any(|a| a == "--dir") {
        script_project(".");
    }
}

#[test]
fn documented_examples_pass_argument_validation() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join("cache");
    let fixtures = seed_cache(&cache);

    let mut failures = Vec::new();
    let mut ran = 0usize;
    for file in doc_files() {
        let text = std::fs::read_to_string(&file).unwrap();
        let rel = file
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string();
        for example in examples(&text) {
            let (kind, args) = match plan(&example, &fixtures) {
                Ok(Plan::Run(kind, args)) => (kind, args),
                Ok(Plan::Skip) => continue,
                Err(e) => {
                    failures.push(format!("{rel}: {example}\n    {e}"));
                    continue;
                }
            };
            let cwd = tempfile::tempdir().unwrap();
            create_inputs(cwd.path(), &args);
            let out = Command::cargo_bin("gwsr")
                .unwrap()
                .env_clear()
                .env("GWSR_CONFIG_DIR", home.path())
                .env("GWSR_CACHE_DIR", &cache)
                .env("HOME", home.path())
                // Nothing here may reach the network.
                .env("HTTPS_PROXY", "http://127.0.0.1:9")
                .env("HTTP_PROXY", "http://127.0.0.1:9")
                .env("ALL_PROXY", "http://127.0.0.1:9")
                .current_dir(cwd.path())
                .args(&args)
                .write_stdin(STDIN)
                .timeout(Duration::from_secs(30))
                .output()
                .unwrap();
            ran += 1;
            let stderr = String::from_utf8_lossy(&out.stderr);
            let ok = match (kind, out.status.code()) {
                (_, Some(0 | 2)) => true,
                (Kind::Synopsis, Some(3)) => !CLAP_ERRORS.iter().any(|e| stderr.contains(e)),
                _ => false,
            };
            if !ok {
                failures.push(format!(
                    "{rel}: {example}\n    ran {args:?}\n    exit {:?}: {}",
                    out.status.code(),
                    stderr.trim()
                ));
            }
        }
    }
    assert!(
        ran > 300,
        "only {ran} examples ran; is the extraction broken?"
    );
    assert!(
        failures.is_empty(),
        "{} documented example(s) are rejected by gwsr:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
