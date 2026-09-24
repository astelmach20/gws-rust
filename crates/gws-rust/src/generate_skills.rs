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

//! Generates SKILL.md files from the CLI's own clap metadata.
//!
//! Usage: `gwsr dev generate-skills --output-dir <DIR> [--filter NAME]... [--index PATH]`

use crate::commands;
use crate::discovery;
use crate::error::GwsError;
use crate::services;
use clap::Command;
use std::path::Path;

const PERSONAS_TOML: &str = include_str!("../registry/personas.toml");
const RECIPES_TOML: &str = include_str!("../registry/recipes.toml");

/// Methods blocked from skill generation.
/// Format: (service_alias, resource, method).
const BLOCKED_METHODS: &[(&str, &str, &str)] = &[
    ("drive", "files", "delete"),
    ("drive", "files", "emptyTrash"),
    ("drive", "drives", "delete"),
    ("drive", "teamdrives", "delete"),
    ("people", "people", "deleteContact"),
    ("people", "people", "batchDeleteContacts"),
];

#[derive(serde::Deserialize)]
struct PersonaRegistry {
    personas: Vec<PersonaEntry>,
}

#[derive(serde::Deserialize)]
struct PersonaEntry {
    name: String,
    title: String,
    description: String,
    services: Vec<String>,
    workflows: Vec<String>,
    instructions: Vec<String>,
    #[serde(default)]
    tips: Vec<String>,
}

#[derive(serde::Deserialize)]
struct RecipeRegistry {
    recipes: Vec<RecipeEntry>,
}

#[derive(serde::Deserialize)]
struct RecipeEntry {
    name: String,
    title: String,
    description: String,
    category: String,
    services: Vec<String>,
    steps: Vec<String>,
    caution: Option<String>,
}

struct SkillIndexEntry {
    name: String,
    description: String,
    category: String,
}

/// Options for `gwsr dev generate-skills`.
#[derive(Debug, Clone, clap::Args)]
pub struct GenerateSkillsArgs {
    /// Directory the skill folders are written into (relative paths resolve against the current directory).
    #[arg(long, value_name = "DIR", required = true)]
    pub output_dir: String,

    /// Only generate the named skills. Repeatable. Each value must match exactly:
    /// a service alias (`drive`, also emits its helpers), a helper key
    /// (`gmail-send`), `shared`, `personas`, `recipes`, `persona-<name>` or
    /// `recipe-<name>`.
    #[arg(long = "filter", value_name = "NAME")]
    pub filters: Vec<String>,

    /// Also write a Markdown skills index to this path (e.g. docs/skills.md).
    #[arg(long, value_name = "PATH")]
    pub index: Option<String>,
}

/// Exact-match skill filter built from repeated `--filter` values.
struct SkillFilter(Vec<String>);

impl SkillFilter {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn has(&self, name: &str) -> bool {
        self.0.is_empty() || self.0.iter().any(|f| f == name)
    }
}

fn io_err(context: String, e: std::io::Error) -> GwsError {
    GwsError::from(anyhow::Error::new(e).context(context))
}

/// Entry point for `gwsr dev generate-skills`.
///
/// Returns a JSON summary of what was written (printed by `main`):
/// `{"output_dir": .., "index": ..|null, "count": N, "skills": [{"name", "category", "path"}],
/// "pruned": [..], "unmanaged": [..]}`.
///
/// With `dry_run` (the global `--dry-run`) nothing is created, written or
/// deleted: the summary gains `"dry_run": true`, `skills` lists what would be
/// written and `wouldPrune` replaces `pruned`.
pub async fn handle_generate_skills(
    args: &GenerateSkillsArgs,
    dry_run: bool,
) -> Result<serde_json::Value, GwsError> {
    // Validate output_dir to prevent path traversal
    let output_path_buf = crate::validate::validate_safe_output_dir(&args.output_dir)?;
    let output_path = output_path_buf.as_path();
    let index_path = match &args.index {
        Some(p) => Some(crate::validate::validate_safe_file_path(p, "--index")?),
        None => None,
    };
    let filter = SkillFilter(args.filters.iter().map(|f| f.trim().to_string()).collect());
    let mut index: Vec<SkillIndexEntry> = Vec::new();
    let mut matched: std::collections::HashSet<String> = std::collections::HashSet::new();

    if filter.has("shared") {
        matched.insert("shared".to_string());
        generate_shared_skill(output_path, dry_run)?;
        index.push(SkillIndexEntry {
            name: "gwsr-shared".to_string(),
            description:
                "gwsr CLI: Shared patterns for authentication, global flags, and output formatting."
                    .to_string(),
            category: "service".to_string(),
        });
    }

    for entry in services::SERVICES {
        let alias = entry.aliases[0];
        let skill_name = format!("gwsr-{alias}");
        let emit_service = filter.has(alias);

        // Skip the (network) discovery fetch entirely when nothing from this
        // service can be selected.
        let helper_prefix = format!("{alias}-");
        let wants_any_helper = filter.is_empty()
            || emit_service
            || filter.0.iter().any(|f| f.starts_with(&helper_prefix));
        if !wants_any_helper {
            continue;
        }

        tracing::info!(
            service = alias,
            api = entry.api_name,
            version = entry.version,
            "generating skills"
        );

        // Synthetic services (no Discovery doc) use an empty RestDescription
        let doc = if entry.api_name == "workflow" {
            discovery::RestDescription {
                name: "workflow".to_string(),
                title: Some("Workflow".to_string()),
                description: Some(entry.description.to_string()),
                ..Default::default()
            }
        } else {
            discovery::fetch_discovery_document(entry.api_name, entry.version)
                .await
                .map_err(|e| {
                    GwsError::Discovery(format!(
                        "failed to fetch the Discovery document for {alias}: {e:#}"
                    ))
                })?
        };

        // Derive product name from Discovery title (e.g. "Google Drive API" -> "Google Drive")
        let product_name = product_name_from_title(doc.title.as_deref().unwrap_or(alias));

        // Build the CLI tree (includes helpers)
        let cli = commands::build_cli(&doc);

        // Collect helper commands (start with '+') and resource commands
        let (helpers, resources): (Vec<&Command>, Vec<&Command>) = cli
            .get_subcommands()
            .partition(|sub| sub.get_name().starts_with('+'));

        if emit_service {
            matched.insert(alias.to_string());
            let service_md =
                render_service_skill(alias, entry, &helpers, &resources, &product_name, &doc);
            write_skill(output_path, &skill_name, &service_md, dry_run)?;
            index.push(SkillIndexEntry {
                name: skill_name.clone(),
                description: service_description(&product_name, entry.description),
                category: "service".to_string(),
            });
        }

        // Generate per-helper skills
        for helper in &helpers {
            let helper_name = helper.get_name();
            // +triage -> triage
            let short = helper_name.trim_start_matches('+');
            let helper_key = format!("{alias}-{short}");

            if emit_service || filter.has(&helper_key) {
                matched.insert(helper_key.clone());
                let helper_skill_name = format!("gwsr-{helper_key}");
                let about_raw = helper.get_about().map(|s| s.to_string());
                let about_raw = about_raw.as_deref().unwrap_or("");
                let about_clean = about_raw.strip_prefix("[Helper] ").unwrap_or(about_raw);
                let helper_md =
                    render_helper_skill(alias, helper_name, helper, entry, &product_name);
                write_skill(output_path, &helper_skill_name, &helper_md, dry_run)?;
                index.push(SkillIndexEntry {
                    name: helper_skill_name,
                    description: truncate_desc(&format!(
                        "{}: {}",
                        product_name,
                        capitalize_first(about_clean)
                    )),
                    category: "helper".to_string(),
                });
            }
        }
    }

    let all_personas = filter.has("personas");
    let registry = toml::from_str::<PersonaRegistry>(PERSONAS_TOML).map_err(|e| {
        GwsError::from(anyhow::Error::new(e).context("embedded registry/personas.toml is invalid"))
    })?;
    if all_personas {
        matched.insert("personas".to_string());
    }
    for persona in registry.personas {
        let name = format!("persona-{}", persona.name);
        if all_personas || filter.0.contains(&name) {
            matched.insert(name.clone());
            let md = render_persona_skill(&persona);
            write_skill(output_path, &name, &md, dry_run)?;
            index.push(SkillIndexEntry {
                name: name.clone(),
                description: truncate_desc(&persona.description),
                category: "persona".to_string(),
            });
        }
    }

    let all_recipes = filter.has("recipes");
    let registry = toml::from_str::<RecipeRegistry>(RECIPES_TOML).map_err(|e| {
        GwsError::from(anyhow::Error::new(e).context("embedded registry/recipes.toml is invalid"))
    })?;
    if all_recipes {
        matched.insert("recipes".to_string());
    }
    for recipe in registry.recipes {
        let name = format!("recipe-{}", recipe.name);
        if all_recipes || filter.0.contains(&name) {
            matched.insert(name.clone());
            let md = render_recipe_skill(&recipe);
            write_skill(output_path, &name, &md, dry_run)?;
            index.push(SkillIndexEntry {
                name: name.clone(),
                description: truncate_desc(&recipe.description),
                category: "recipe".to_string(),
            });
        }
    }

    // Every filter value must have selected something: a typo must not
    // silently produce an empty run.
    let unmatched: Vec<&String> = filter.0.iter().filter(|f| !matched.contains(*f)).collect();
    if !unmatched.is_empty() {
        return Err(GwsError::Validation(format!(
            "--filter matched no skill: {}",
            unmatched
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    if let Some(path) = &index_path {
        write_skills_index(&index, path, dry_run)?;
    }

    // Only a full run knows every skill that should exist; a filtered run
    // writes a subset and must not treat the rest as stale.
    let tidy = if filter.is_empty() {
        let produced: std::collections::HashSet<&str> =
            index.iter().map(|e| e.name.as_str()).collect();
        prune_stale_skills(output_path, &produced, dry_run)?
    } else {
        Tidy::default()
    };

    tracing::info!(count = index.len(), pruned = tidy.pruned.len(), dir = %output_path.display(), "skills written");
    Ok(skills_summary(
        output_path,
        index_path.as_deref(),
        &index,
        &tidy,
        dry_run,
    ))
}

/// First line after the front matter of every generated `SKILL.md`: marks the
/// directory as owned by the generator, so a later run may delete it once the
/// skill is no longer produced.
/// (It names no command: skills must not steer agents toward the generator.)
const GENERATED_MARKER: &str = "<!-- gwsr generated skill: do not edit by hand -->";

/// Insert [`GENERATED_MARKER`] right after the YAML front matter.
fn with_generated_marker(content: &str) -> Result<String, GwsError> {
    let end = content
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n"))
        .map(|i| 4 + i + "\n---\n".len())
        .ok_or_else(|| {
            GwsError::other(anyhow::anyhow!(
                "internal error: generated skill has no YAML front matter"
            ))
        })?;
    let (front, body) = content.split_at(end);
    Ok(format!("{front}{GENERATED_MARKER}\n{body}"))
}

/// Directories removed or left alone by [`prune_stale_skills`].
#[derive(Debug, Default)]
struct Tidy {
    /// Generated skill directories that are no longer produced: deleted, or
    /// on a dry run the ones that would be deleted.
    pruned: Vec<String>,
    /// Directories in the output dir that this run did not produce and that
    /// carry no generator marker; never touched, reported so leftovers from
    /// hand edits or older generators are visible.
    unmanaged: Vec<String>,
}

/// Delete generated skill directories under `base` that `produced` no longer
/// contains. A directory is deleted only when its sole entry is a `SKILL.md`
/// carrying [`GENERATED_MARKER`]; a marked directory holding anything else is
/// an error rather than a partial delete. With `dry_run` the same checks run
/// but nothing is deleted.
fn prune_stale_skills(
    base: &Path,
    produced: &std::collections::HashSet<&str>,
    dry_run: bool,
) -> Result<Tidy, GwsError> {
    let mut tidy = Tidy::default();
    let entries = match std::fs::read_dir(base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(tidy),
        Err(e) => return Err(io_err(format!("failed to list {}", base.display()), e)),
    };
    let mut stale = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_err(format!("failed to list {}", base.display()), e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| io_err(format!("failed to inspect {}", entry.path().display()), e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !file_type.is_dir() || produced.contains(name.as_str()) {
            continue;
        }
        stale.push((name, entry.path()));
    }
    stale.sort();
    for (name, dir) in stale {
        let skill = dir.join("SKILL.md");
        let owned = match std::fs::read_to_string(&skill) {
            Ok(text) => text.lines().any(|l| l == GENERATED_MARKER),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_err(format!("failed to read {}", skill.display()), e)),
        };
        if !owned {
            tidy.unmanaged.push(name);
            continue;
        }
        let mut others = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| io_err(format!("failed to list {}", dir.display()), e))?
        {
            let entry =
                entry.map_err(|e| io_err(format!("failed to list {}", dir.display()), e))?;
            if entry.file_name() != "SKILL.md" {
                others.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        if !others.is_empty() {
            return Err(GwsError::Validation(format!(
                "stale generated skill {} also holds files the generator did not write ({}); \
                 move them out or delete the directory, then rerun",
                dir.display(),
                others.join(", ")
            )));
        }
        if !dry_run {
            std::fs::remove_file(&skill)
                .map_err(|e| io_err(format!("failed to remove {}", skill.display()), e))?;
            std::fs::remove_dir(&dir)
                .map_err(|e| io_err(format!("failed to remove {}", dir.display()), e))?;
        }
        tidy.pruned.push(name);
    }
    Ok(tidy)
}

fn skills_summary(
    output_path: &Path,
    index_path: Option<&Path>,
    index: &[SkillIndexEntry],
    tidy: &Tidy,
    dry_run: bool,
) -> serde_json::Value {
    let skills: Vec<serde_json::Value> = index
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "category": e.category,
                "path": output_path.join(&e.name).join("SKILL.md").display().to_string(),
            })
        })
        .collect();
    let mut summary = serde_json::json!({
        "output_dir": output_path.display().to_string(),
        "index": index_path.map(|p| p.display().to_string()),
        "count": skills.len(),
        "skills": skills,
        "unmanaged": tidy.unmanaged,
    });
    let (pruned_key, extra) = if dry_run {
        (
            "wouldPrune",
            Some(("dry_run", serde_json::Value::Bool(true))),
        )
    } else {
        ("pruned", None)
    };
    if let Some(map) = summary.as_object_mut() {
        map.insert(pruned_key.to_string(), serde_json::json!(tidy.pruned));
        if let Some((k, v)) = extra {
            map.insert(k.to_string(), v);
        }
    }
    summary
}

/// Write one skill; on a dry run only render it (so render errors still
/// surface) and touch nothing on disk.
fn write_skill(base: &Path, name: &str, content: &str, dry_run: bool) -> Result<(), GwsError> {
    let content = with_generated_marker(content)?;
    if dry_run {
        return Ok(());
    }
    let dir = base.join(name);
    std::fs::create_dir_all(&dir)
        .map_err(|e| io_err(format!("failed to create directory {}", dir.display()), e))?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, content)
        .map_err(|e| io_err(format!("failed to write {}", path.display()), e))?;
    Ok(())
}

fn write_skills_index(
    entries: &[SkillIndexEntry],
    path: &Path,
    dry_run: bool,
) -> Result<(), GwsError> {
    let mut out = String::new();
    out.push_str("# Skills Index\n\n");
    out.push_str("> Auto-generated by `gwsr dev generate-skills`. Do not edit manually.\n\n");

    let sections = [
        (
            "service",
            "## Services",
            "Core Google Workspace API skills.",
        ),
        (
            "helper",
            "## Helpers",
            "Shortcut commands for common operations.",
        ),
        ("persona", "## Personas", "Role-based skill bundles."),
        (
            "recipe",
            "## Recipes",
            "Multi-step task sequences with real commands.",
        ),
    ];

    for (cat, heading, subtitle) in &sections {
        let items: Vec<&SkillIndexEntry> = entries.iter().filter(|e| e.category == *cat).collect();
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("{heading}\n\n{subtitle}\n\n"));
        out.push_str("| Skill | Description |\n|-------|-------------|\n");
        for item in &items {
            out.push_str(&format!(
                "| [{}](../skills/{}/SKILL.md) | {} |\n",
                item.name, item.name, item.description
            ));
        }
        out.push('\n');
    }

    if dry_run {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            io_err(
                format!("failed to create directory {}", parent.display()),
                e,
            )
        })?;
    }
    std::fs::write(path, &out).map_err(|e| {
        io_err(
            format!("failed to write skills index {}", path.display()),
            e,
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

/// Returns true if a (service, resource, method) triple is blocked.
fn is_blocked_method(alias: &str, resource: &str, method: &str) -> bool {
    BLOCKED_METHODS
        .iter()
        .any(|(s, r, m)| *s == alias && *r == resource && *m == method)
}

fn render_service_skill(
    alias: &str,
    entry: &services::ServiceEntry,
    helpers: &[&Command],
    resources: &[&Command],
    product_name: &str,
    doc: &crate::discovery::RestDescription,
) -> String {
    let mut out = String::new();

    let trigger_desc = service_description(product_name, entry.description);

    // Frontmatter
    out.push_str(&format!(
        r#"---
name: gwsr-{alias}
description: "{trigger_desc}"
metadata:
  version: {version}
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr {alias} --help"
---

"#,
        version = env!("CARGO_PKG_VERSION"),
    ));

    // Title
    let api_version = entry.version;
    out.push_str(&format!("# {alias} ({api_version})\n\n"));

    out.push_str(
        "> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.\n\n",
    );

    out.push_str(&format!(
        "```bash\ngwsr {alias} <resource> <method> [flags]\n```\n\n",
    ));

    // Helper commands
    if !helpers.is_empty() {
        out.push_str("## Helper Commands\n\n");
        out.push_str("| Command | Description |\n");
        out.push_str("|---------|-------------|\n");
        for h in helpers {
            let name = h.get_name();
            let short = name.trim_start_matches('+');
            let about = h.get_about().map(|s| s.to_string()).unwrap_or_default();
            // Strip the "[Helper] " prefix if present
            let about = about.strip_prefix("[Helper] ").unwrap_or(&about);
            out.push_str(&format!(
                "| [`{name}`](../gwsr-{alias}-{short}/SKILL.md) | {about} |\n"
            ));
        }
        out.push('\n');
    }

    // API resources
    if !resources.is_empty() {
        out.push_str("## API Resources\n\n");
        for res in resources {
            let res_name = res.get_name();
            let methods: Vec<String> = res
                .get_subcommands()
                .filter(|m| !is_blocked_method(alias, res_name, m.get_name()))
                .map(|m| {
                    let mname = m.get_name().to_string();
                    // Use full description from discovery doc (with higher limit)
                    // instead of the CLI-truncated about text.
                    let mabout =
                        lookup_method_description(doc, res_name, &mname).unwrap_or_else(|| {
                            m.get_about().map(|s| s.to_string()).unwrap_or_default()
                        });
                    format!("  - `{mname}` — {mabout}")
                })
                .collect();

            if methods.is_empty() {
                // Might have sub-resources, list them
                let subs: Vec<String> = res
                    .get_subcommands()
                    .filter(|s| s.get_subcommands().next().is_some())
                    .map(|s| format!("  - `{}`", s.get_name()))
                    .collect();
                if !subs.is_empty() {
                    out.push_str(&format!("### {res_name}\n\n"));
                    for s in subs {
                        out.push_str(&s);
                        out.push('\n');
                    }
                    out.push('\n');
                }
            } else {
                out.push_str(&format!("### {res_name}\n\n"));
                for m in &methods {
                    out.push_str(m);
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }

    // Discovering commands section
    out.push_str("## Discovering Commands\n\n");
    out.push_str("Before calling any API method, inspect it:\n\n");
    out.push_str(&format!("```bash\n# Browse resources and methods\ngwsr {alias} --help\n\n# Inspect a method's required params, types, and defaults\ngwsr schema {alias}.<resource>.<method>\n```\n\n"));
    out.push_str("Use `gwsr schema` output to build your `--params` and `--json` flags.\n\n");

    out
}

fn render_helper_skill(
    alias: &str,
    cmd_name: &str,
    cmd: &Command,
    entry: &services::ServiceEntry,
    product_name: &str,
) -> String {
    let mut out = String::new();

    let about_raw = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
    let about = about_raw.strip_prefix("[Helper] ").unwrap_or(&about_raw);

    let short = cmd_name.trim_start_matches('+');
    let capitalized_about = capitalize_first(about);
    let trigger_desc = truncate_desc(&format!("{}: {}", product_name, capitalized_about));

    // Determine if write command
    let is_write = matches!(
        short,
        "send"
            | "write"
            | "upload"
            | "push"
            | "insert"
            | "append"
            | "create-template"
            | "subscribe"
    );
    let category = if alias == "modelarmor" {
        "security"
    } else {
        "productivity"
    };

    // Frontmatter
    out.push_str(&format!(
        r#"---
name: gwsr-{alias}-{short}
description: "{trigger_desc}"
metadata:
  version: {version}
  openclaw:
    category: "{category}"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr {alias} {cmd_name} --help"
---

"#,
        version = env!("CARGO_PKG_VERSION"),
    ));

    // Title
    out.push_str(&format!("# {alias} {cmd_name}\n\n"));

    out.push_str(
        "> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.\n\n",
    );

    out.push_str(&format!("{about}\n\n"));

    // Usage
    out.push_str("## Usage\n\n");
    out.push_str(&format!("```bash\ngwsr {alias} {cmd_name}"));

    // Show required args inline
    let args: Vec<_> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help")
        .collect();
    for arg in &args {
        if arg.is_required_set() {
            if let Some(long) = arg.get_long() {
                let val_name = arg
                    .get_value_names()
                    .and_then(|v| v.first())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "VALUE".to_string());
                out.push_str(&format!(" --{long} <{val_name}>"));
            } else {
                let id = arg.get_id().as_str();
                out.push_str(&format!(" <{id}>"));
            }
        }
    }

    out.push_str("\n```\n\n");

    // Flags table
    if !args.is_empty() {
        out.push_str("## Flags\n\n");
        out.push_str("| Flag | Required | Default | Description |\n");
        out.push_str("|------|----------|---------|-------------|\n");

        for arg in &args {
            let flag = if let Some(long) = arg.get_long() {
                format!("`--{long}`")
            } else {
                format!("`<{}>`", arg.get_id().as_str())
            };

            let required = if arg.is_required_set() { "✓" } else { "—" };

            // Get default value
            let default = arg
                .get_default_values()
                .first()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_else(|| "—".to_string());

            let help = arg
                .get_help()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "—".to_string());

            out.push_str(&format!("| {flag} | {required} | {default} | {help} |\n"));
        }
        out.push('\n');
    }

    // After-help (examples, tips) — format as proper markdown
    if let Some(after) = cmd.get_after_help() {
        let after_str = after.to_string();
        if !after_str.is_empty() {
            let mut in_examples = false;
            let mut in_tips = false;
            let mut examples = Vec::new();
            let mut tips = Vec::new();

            for line in after_str.lines() {
                let trimmed = line.trim();
                if trimmed == "EXAMPLES:" {
                    in_examples = true;
                    in_tips = false;
                    continue;
                }
                if trimmed == "TIPS:" {
                    in_tips = true;
                    in_examples = false;
                    continue;
                }
                if in_examples && !trimmed.is_empty() {
                    examples.push(trimmed.to_string());
                }
                if in_tips && !trimmed.is_empty() {
                    tips.push(trimmed.to_string());
                }
            }

            if !examples.is_empty() {
                out.push_str("## Examples\n\n```bash\n");
                for ex in &examples {
                    out.push_str(ex);
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }

            if !tips.is_empty() {
                out.push_str("## Tips\n\n");
                for tip in &tips {
                    out.push_str(&format!("- {tip}\n"));
                }
                out.push('\n');
            }
        }
    }

    // Write warning
    if is_write {
        out.push_str("> [!CAUTION]\n");
        out.push_str("> This is a **write** command — confirm with the user before executing.\n\n");
    }

    // Cross-reference
    out.push_str(&format!(
        "## See Also\n\n- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth\n- [gwsr-{alias}](../gwsr-{alias}/SKILL.md) — All {} commands\n",
        entry.description.to_lowercase(),
    ));

    out
}

fn generate_shared_skill(base: &Path, dry_run: bool) -> Result<(), GwsError> {
    let content = r#"---
name: gwsr-shared
description: "gwsr CLI: Shared patterns for authentication, global flags, and output formatting."
metadata:
  version: __VERSION__
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
---

# gwsr — Shared Reference

## Installation

The `gwsr` binary must be on `$PATH`. See the project README for install options.

## Authentication

```bash
# Browser-based OAuth (read-only scopes by default; add --write or --scopes for more)
gwsr auth login
gwsr auth login --write -s drive,gmail     # read-write for selected services
gwsr auth login --no-localhost             # headless/SSH: paste the redirected URL back

# Service account or authorized-user JSON (no login needed)
export GWSR_CREDENTIALS_FILE=/path/to/key.json

# Check what is active and which scopes were granted
gwsr auth status
```

Use `--profile NAME` (or `GWSR_PROFILE`) to pick a credential profile, and
`--impersonate USER` (service accounts with domain-wide delegation) to act as a user.

## Global Flags

| Flag | Description |
|------|-------------|
| `--format <FORMAT>` | Output format: `json` (default), `table`, `yaml`, `csv` |
| `--jq <EXPR>` | Filter the output with a jq expression; string results print raw |
| `--columns <A,B>` | Columns to show for `table` / `csv` output (dot paths for nested fields) |
| `--compact` / `--pretty` | Force compact or pretty JSON (default: compact when piped, pretty on a terminal) |
| `--dry-run` | Validate locally and print the request without sending it |
| `--sanitize <TEMPLATE>` | Screen responses through a Model Armor template |
| `--api-version <V>` | Override the API version (also `<service>:<version>`, e.g. `youtube:v3`) |
| `--profile <NAME>` | Credential profile |
| `--impersonate <EMAIL>` | Service accounts only: act as this user (domain-wide delegation) |
| `-v` / `-q` | More / less diagnostic logging on stderr |

Run `gwsr commands` for a machine-readable (JSON) inventory of every service, resource,
method and helper with its flags.

## Output, Errors and Exit Codes

stdout carries only results: JSON by default (NDJSON for `--page-all`, `--page-items`
and streaming helpers). Logs and progress go to stderr. On failure stdout is empty and
stderr holds one JSON object: `{"error":{"code","message","reason","retryable","hint"?}}`
(human-readable text with a non-JSON `--format`).

Exit codes: `0` success, `1` API error (permanent), `2` auth, `3` validation,
`4` discovery, `5` internal, `6` API error that is safe to retry (429 / 5xx / rate limit),
`7` confirmation required (nothing was sent; re-run with `--yes` once the user agrees),
`8` configuration (`configError`), `9` credential store (`credentialStoreError`: keyring or
stored credentials), `10` network (`networkError`: no response; a non-idempotent call may
have been applied, so check before retrying), `11` output blocked by Model Armor
(`sanitizationBlocked`: `--sanitize` in block mode; the request itself succeeded).

## CLI Syntax

```bash
gwsr <service> <resource> [sub-resource] <method> [flags]
```

### Method Flags

| Flag | Description |
|------|-------------|
| `--params '{"key": "val"}'` | URL/query parameters (`@file` or `-` for stdin also work) |
| `--json '{"key": "val"}'` | Request body (`@file` or `-` for stdin also work) |
| `--fields <MASK>` | Partial response, e.g. `'files(id,name)'` — keeps output small |
| `-o, --output <PATH>` | Write the response payload to a file (required for binary media) |
| `--upload <PATH>` | Upload file content (resumable above 5 MiB) |
| `--page-all` | Fetch every page, one JSON line per page (NDJSON) |
| `--page-items[=FIELD]` | Fetch every page, one JSON line per item |
| `--page-limit <N>` | Stop after N pages (default: unlimited) |
| `--wait` | Poll a returned long-running operation until it finishes |
| `-y, --yes` | Confirm a destructive method (required when not on a terminal) |

Unknown `--params` names and body fields are rejected; run `gwsr schema <service>.<resource>.<method>`
to see what a method accepts.

## Security Rules

- **Never** output secrets (API keys, tokens) directly
- **Always** confirm with the user before executing write/delete commands; only then pass `--yes`
- Prefer `--dry-run` to preview mutating requests
- Use `--sanitize` for PII/content safety screening

## Shell Tips

- **Quote arguments containing `!` with single quotes.** Sheet ranges like `Sheet1!A1` contain `!`, which interactive bash (and zsh with `BANG_HIST`) history-expands inside double quotes. Single quotes are never expanded:
  ```bash
  # CORRECT: single quotes are safe in bash and zsh
  gwsr sheets +read --spreadsheet-id ID --range 'Sheet1!A1:D10'

  # WRONG: interactive bash expands `!A1` inside double quotes
  gwsr sheets +read --spreadsheet-id ID --range "Sheet1!A1:D10"
  ```
- **JSON with double quotes:** Wrap `--params` and `--json` values in single quotes so the shell does not interpret the inner double quotes:
  ```bash
  gwsr drive files list --params '{"pageSize": 5}'
  ```
"#
    .replace("__VERSION__", env!("CARGO_PKG_VERSION"));

    write_skill(base, "gwsr-shared", &content, dry_run)
}

fn render_persona_skill(persona: &PersonaEntry) -> String {
    let mut out = String::new();

    // Block-style YAML for skills array
    let required_skills = persona
        .services
        .iter()
        .map(|s| format!("        - gwsr-{s}"))
        .collect::<Vec<_>>()
        .join("\n");

    let trigger_desc = truncate_desc(&persona.description);

    out.push_str(&format!(
        r#"---
name: persona-{name}
description: "{trigger_desc}"
metadata:
  version: {version}
  openclaw:
    category: "persona"
    requires:
      bins:
        - gwsr
      skills:
{skills}
---

# {title}

> **PREREQUISITE:** Load the following utility skills to operate as this persona: {skills_list}

{description}

## Relevant Workflows
{workflows}

## Instructions
"#,
        name = persona.name,
        description = persona.description,
        title = persona.title,
        skills = required_skills,
        skills_list = persona
            .services
            .iter()
            .map(|s| format!("`gwsr-{s}`"))
            .collect::<Vec<_>>()
            .join(", "),
        version = env!("CARGO_PKG_VERSION"),
        workflows = persona
            .workflows
            .iter()
            .map(|w| format!("- `gwsr workflow {w}`"))
            .collect::<Vec<_>>()
            .join("\n")
    ));

    for inst in &persona.instructions {
        out.push_str(&format!("- {inst}\n"));
    }
    out.push('\n');

    if !persona.tips.is_empty() {
        out.push_str("## Tips\n");
        for tip in &persona.tips {
            out.push_str(&format!("- {tip}\n"));
        }
        out.push('\n');
    }

    out
}

fn render_recipe_skill(recipe: &RecipeEntry) -> String {
    let mut out = String::new();

    let required_skills = recipe
        .services
        .iter()
        .map(|s| format!("        - gwsr-{s}"))
        .collect::<Vec<_>>()
        .join("\n");

    let trigger_desc = truncate_desc(&recipe.description);

    out.push_str(&format!(
        r#"---
name: recipe-{name}
description: "{trigger_desc}"
metadata:
  version: {version}
  openclaw:
    category: "recipe"
    domain: "{category}"
    requires:
      bins:
        - gwsr
      skills:
{skills}
---

# {title}

> **PREREQUISITE:** Load the following skills to execute this recipe: {skills_list}

{description}

"#,
        name = recipe.name,
        description = recipe.description,
        title = recipe.title,
        category = recipe.category,
        version = env!("CARGO_PKG_VERSION"),
        skills = required_skills,
        skills_list = recipe
            .services
            .iter()
            .map(|s| format!("`gwsr-{s}`"))
            .collect::<Vec<_>>()
            .join(", "),
    ));

    if let Some(caution) = &recipe.caution {
        out.push_str(&format!("> [!CAUTION]\n> {caution}\n\n"));
    }

    out.push_str("## Steps\n\n");
    for (i, step) in recipe.steps.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, step));
    }
    out.push('\n');

    out
}

fn truncate_desc(desc: &str) -> String {
    let mut s = desc.replace('"', "'").trim().to_string();
    // Capitalize first letter
    if let Some(first) = s.get(0..1) {
        s = format!("{}{}", first.to_uppercase(), &s[1..]);
    }
    // Delegate to shared truncation logic
    s = crate::text::truncate_description(&s, crate::text::FRONTMATTER_DESCRIPTION_LIMIT, true);
    // Ensure trailing period
    if !s.ends_with('.') && !s.ends_with('…') {
        s.push('.');
    }
    s
}

/// Looks up a method's full description from the Discovery Document and
/// truncates it at the skill-body limit (longer than CLI help).
fn lookup_method_description(
    doc: &crate::discovery::RestDescription,
    resource_name: &str,
    method_name: &str,
) -> Option<String> {
    let resource = doc.resources.get(resource_name)?;
    // Try direct method lookup first
    if let Some(method) = resource.methods.get(method_name)
        && let Some(desc) = &method.description
    {
        return Some(crate::text::truncate_description(
            desc,
            crate::text::SKILL_BODY_DESCRIPTION_LIMIT,
            false,
        ));
    }
    // For sub-resources listed as methods in the clap tree, return None
    // (they show as "Operations on the 'X' resource" which is fine)
    None
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
    }
}

fn product_name_from_title(title: &str) -> String {
    // Discovery titles are like "Google Drive API", "Gmail API", "Model Armor API"
    // Strip " API" suffix to get the product name
    let name = title.strip_suffix(" API").unwrap_or(title).trim();
    if name.is_empty() {
        return "Unknown".to_string();
    }
    // Prepend "Google" if not already present (most Workspace products are "Google X")
    // Skip for standalone brands like "Gmail"
    if !name.starts_with("Google") && !name.starts_with("Gmail") {
        // Workspace management tools get "Google Workspace" prefix
        let is_workspace_mgmt =
            name.contains("Admin") || name.contains("Enterprise") || name.contains("Reseller");
        if is_workspace_mgmt {
            return format!("Google Workspace {name}");
        }
        return format!("Google {name}");
    }
    name.to_string()
}

fn service_description(product_name: &str, discovery_desc: &str) -> String {
    // If the description already mentions the product name, use it as-is
    let desc_lower = discovery_desc.to_lowercase();
    let name_lower = product_name.to_lowercase();
    if desc_lower.contains(&name_lower) {
        return truncate_desc(discovery_desc);
    }

    // Prepend the product name
    truncate_desc(&format!("{product_name}: {discovery_desc}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers;
    use crate::services;
    use clap::Command;
    use std::collections::HashSet;

    #[test]
    fn test_registry_references() {
        let personas: PersonaRegistry = toml::from_str(PERSONAS_TOML).expect("valid personas toml");
        let recipes: RecipeRegistry = toml::from_str(RECIPES_TOML).expect("valid recipes toml");

        // Valid services mapped by api_name or alias
        let all_services = services::SERVICES;
        let mut valid_services = HashSet::new();
        for s in all_services {
            valid_services.insert(s.api_name);
            for alias in s.aliases {
                valid_services.insert(*alias);
            }
        }
        // Workflows are synthetic and technically a service, so add it
        valid_services.insert("workflow");

        // Valid workflows
        let wf_helper = helpers::get_helper("workflow").expect("workflow helper missing");
        let mut cli = Command::new("test");
        let doc = crate::discovery::RestDescription::default();
        cli = wf_helper.inject_commands(cli, &doc);
        let valid_workflows: HashSet<_> = cli
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();

        // Validate personas
        for p in personas.personas {
            for s in &p.services {
                assert!(
                    valid_services.contains(s.as_str()),
                    "Persona '{}' refs invalid service '{}'",
                    p.name,
                    s
                );
            }
            for w in &p.workflows {
                assert!(
                    valid_workflows.contains(w.as_str()),
                    "Persona '{}' refs invalid workflow '{}'",
                    p.name,
                    w
                );
            }
        }

        // Validate recipes
        for r in recipes.recipes {
            for s in &r.services {
                assert!(
                    valid_services.contains(s.as_str()),
                    "Recipe '{}' refs invalid service '{}'",
                    r.name,
                    s
                );
            }
        }
    }

    #[test]
    fn skills_summary_lists_written_files() {
        let index = vec![SkillIndexEntry {
            name: "gwsr-shared".into(),
            description: "d".into(),
            category: "service".into(),
        }];
        let tidy = Tidy {
            pruned: vec!["gwsr-old".into()],
            unmanaged: vec!["notes".into()],
        };
        let v = skills_summary(
            Path::new("/out"),
            Some(Path::new("/idx.md")),
            &index,
            &tidy,
            false,
        );
        assert_eq!(v["count"], 1);
        assert_eq!(v["pruned"][0], "gwsr-old");
        assert_eq!(v["unmanaged"][0], "notes");
        assert_eq!(v["output_dir"], "/out");
        assert_eq!(v["index"], "/idx.md");
        assert_eq!(v["skills"][0]["path"], "/out/gwsr-shared/SKILL.md");
        assert!(v.get("dry_run").is_none() && v.get("wouldPrune").is_none());

        let dry = skills_summary(Path::new("/out"), None, &index, &tidy, true);
        assert_eq!(dry["dry_run"], true);
        assert_eq!(dry["wouldPrune"][0], "gwsr-old");
        assert!(dry.get("pruned").is_none());
        assert_eq!(dry["skills"][0]["path"], "/out/gwsr-shared/SKILL.md");
    }

    #[test]
    fn dry_run_writes_no_skill_or_index() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "gwsr-x", "---\nname: gwsr-x\n---\n", true).unwrap();
        assert!(!dir.path().join("gwsr-x").exists());
        // Render errors still surface on a dry run.
        assert!(write_skill(dir.path(), "gwsr-y", "# no front matter", true).is_err());
        let index = dir.path().join("docs/skills.md");
        write_skills_index(&[], &index, true).unwrap();
        assert!(!dir.path().join("docs").exists());
        generate_shared_skill(dir.path(), true).unwrap();
        assert!(!dir.path().join("gwsr-shared").exists());
    }

    #[test]
    fn dry_run_prune_reports_but_deletes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        write_skill(base, "gwsr-stale", "---\nname: s\n---\n", false).unwrap();
        let tidy = prune_stale_skills(base, &std::collections::HashSet::new(), true).unwrap();
        assert_eq!(tidy.pruned, vec!["gwsr-stale"]);
        assert!(base.join("gwsr-stale/SKILL.md").exists());
        // The safety check still applies: a dry run reports the same error.
        std::fs::write(base.join("gwsr-stale/notes.txt"), "mine").unwrap();
        assert!(prune_stale_skills(base, &std::collections::HashSet::new(), true).is_err());
    }

    #[test]
    fn generated_skills_carry_the_marker_after_the_front_matter() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "gwsr-x",
            "---\nname: gwsr-x\n---\n\n# x\n",
            false,
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.path().join("gwsr-x/SKILL.md")).unwrap();
        assert_eq!(
            text,
            format!("---\nname: gwsr-x\n---\n{GENERATED_MARKER}\n\n# x\n")
        );
        assert!(with_generated_marker("# no front matter").is_err());
    }

    #[test]
    fn prune_removes_only_stale_generated_skill_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let skill = "---\nname: s\n---\n\n# s\n";
        write_skill(base, "gwsr-kept", skill, false).unwrap();
        write_skill(base, "gwsr-stale", skill, false).unwrap();
        // Not generated: no marker, or no SKILL.md at all, or not a directory.
        std::fs::create_dir(base.join("hand-written")).unwrap();
        std::fs::write(base.join("hand-written/SKILL.md"), skill).unwrap();
        std::fs::create_dir(base.join("empty")).unwrap();
        std::fs::write(base.join("README.md"), "x").unwrap();

        let produced: std::collections::HashSet<&str> = ["gwsr-kept"].into();
        let tidy = prune_stale_skills(base, &produced, false).unwrap();
        assert_eq!(tidy.pruned, vec!["gwsr-stale"]);
        assert_eq!(tidy.unmanaged, vec!["empty", "hand-written"]);
        assert!(!base.join("gwsr-stale").exists());
        assert!(base.join("gwsr-kept/SKILL.md").exists());
        assert!(base.join("hand-written/SKILL.md").exists());
        assert!(base.join("README.md").exists());
    }

    #[test]
    fn prune_refuses_a_generated_dir_holding_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        write_skill(base, "gwsr-stale", "---\nname: s\n---\n", false).unwrap();
        std::fs::write(base.join("gwsr-stale/notes.txt"), "mine").unwrap();
        let err = prune_stale_skills(base, &std::collections::HashSet::new(), false).unwrap_err();
        assert!(err.to_string().contains("notes.txt"), "{err}");
        assert!(base.join("gwsr-stale/notes.txt").exists());
        assert!(base.join("gwsr-stale/SKILL.md").exists());
    }

    /// CI's leftover check greps for the marker; keep the two in sync.
    #[test]
    fn ci_leftover_check_uses_the_marker() {
        let ci =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
        let ci = std::fs::read_to_string(&ci).unwrap();
        assert!(ci.contains(&format!("'{GENERATED_MARKER}'")));
    }

    #[test]
    fn prune_of_a_missing_output_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let tidy = prune_stale_skills(
            &dir.path().join("nope"),
            &std::collections::HashSet::new(),
            false,
        )
        .unwrap();
        assert!(tidy.pruned.is_empty() && tidy.unmanaged.is_empty());
    }

    #[test]
    fn test_truncate_desc_short() {
        assert_eq!(truncate_desc("hello world"), "Hello world.");
    }

    #[test]
    fn test_truncate_desc_capitalizes() {
        assert_eq!(truncate_desc("lists all files."), "Lists all files.");
    }

    #[test]
    fn test_truncate_desc_replaces_quotes() {
        assert_eq!(
            truncate_desc(r#"Returns a "File" resource."#),
            "Returns a 'File' resource."
        );
    }

    #[test]
    fn test_truncate_desc_truncates_long() {
        let long = "A ".repeat(100); // 200 chars
        let result = truncate_desc(&long);
        assert!(
            result.chars().count() <= crate::text::FRONTMATTER_DESCRIPTION_LIMIT + 2,
            "should respect limit"
        );
    }

    #[test]
    fn test_truncate_desc_adds_period() {
        assert_eq!(truncate_desc("no period"), "No period.");
    }

    #[test]
    fn test_truncate_desc_preserves_existing_period() {
        assert_eq!(truncate_desc("has one."), "Has one.");
    }

    #[test]
    fn test_truncate_desc_ellipsis_no_period() {
        // When truncation produces an ellipsis, don't add a period
        let long = "word ".repeat(50);
        let result = truncate_desc(&long);
        assert!(result.ends_with('…'));
        assert!(!result.ends_with(".…"));
    }

    #[test]
    fn test_lookup_method_description_found() {
        let mut methods = std::collections::HashMap::new();
        methods.insert(
            "list".to_string(),
            crate::discovery::RestMethod {
                description: Some(
                    "Lists all the files. For more details see the docs.".to_string(),
                ),
                http_method: "GET".to_string(),
                path: "files".to_string(),
                ..Default::default()
            },
        );
        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "files".to_string(),
            crate::discovery::RestResource {
                methods,
                ..Default::default()
            },
        );
        let doc = crate::discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };
        let result = lookup_method_description(&doc, "files", "list");
        assert!(result.is_some());
        assert!(result.unwrap().contains("Lists all the files"));
    }

    #[test]
    fn test_lookup_method_description_missing_resource() {
        let doc = crate::discovery::RestDescription {
            name: "drive".to_string(),
            ..Default::default()
        };
        assert!(lookup_method_description(&doc, "missing", "list").is_none());
    }

    #[test]
    fn test_lookup_method_description_missing_method() {
        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "files".to_string(),
            crate::discovery::RestResource::default(),
        );
        let doc = crate::discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };
        assert!(lookup_method_description(&doc, "files", "missing").is_none());
    }

    #[test]
    fn test_lookup_method_description_no_description() {
        let mut methods = std::collections::HashMap::new();
        methods.insert(
            "list".to_string(),
            crate::discovery::RestMethod {
                description: None,
                http_method: "GET".to_string(),
                path: "files".to_string(),
                ..Default::default()
            },
        );
        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "files".to_string(),
            crate::discovery::RestResource {
                methods,
                ..Default::default()
            },
        );
        let doc = crate::discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };
        assert!(lookup_method_description(&doc, "files", "list").is_none());
    }

    #[test]
    fn test_capitalize_first_empty() {
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn test_capitalize_first_basic() {
        assert_eq!(capitalize_first("hello"), "Hello");
    }

    #[test]
    fn test_product_name_from_title_strips_api() {
        assert_eq!(product_name_from_title("Google Drive API"), "Google Drive");
    }

    #[test]
    fn test_product_name_from_title_no_api_suffix() {
        // product_name_from_title prepends "Google" if not already present
        assert_eq!(product_name_from_title("Workspace"), "Google Workspace");
    }

    #[test]
    fn test_product_name_from_title_adds_google() {
        assert_eq!(product_name_from_title("Drive API"), "Google Drive");
    }

    /// Extract the YAML frontmatter (between `---` delimiters) from a skill string.
    fn extract_frontmatter(content: &str) -> &str {
        let content = content.strip_prefix("---").expect("no opening ---");
        let (frontmatter, _) = content.split_once("\n---").expect("no closing ---");
        frontmatter
    }

    /// Asserts that the frontmatter uses block-style YAML sequences.
    ///
    /// Detects flow sequences by checking whether YAML values start with `[`,
    /// rather than looking for brackets anywhere in a line.  This avoids false
    /// positives from string values that legitimately contain brackets
    /// (e.g., `description: 'Note: [INTERNAL] ticket was filed'`).
    fn assert_block_style_sequences(frontmatter: &str) {
        for (i, line) in frontmatter.lines().enumerate() {
            let trimmed = line.trim();
            // Skip lines that don't look like YAML values (e.g., comments, empty)
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // A YAML flow sequence is "key: [...]". Check the value after `:`.
            if let Some(colon_pos) = trimmed.find(':') {
                let value = trimmed[colon_pos + 1..].trim();
                // A flow sequence is not quoted. A quoted string is a scalar.
                let is_quoted = value.starts_with('"') || value.starts_with('\'');
                assert!(
                    is_quoted || !value.starts_with('['),
                    "Flow sequence found on line {} of frontmatter: {:?}\n\
                     Use block-style sequences instead (e.g., `- value`)",
                    i + 1,
                    trimmed
                );
            }
        }
    }

    #[test]
    fn test_service_skill_frontmatter_uses_block_sequences() {
        let entry = &services::SERVICES[0]; // first service
        let doc = crate::discovery::RestDescription {
            name: entry.api_name.to_string(),
            title: Some("Test API".to_string()),
            description: Some(entry.description.to_string()),
            ..Default::default()
        };
        let cli = crate::commands::build_cli(&doc);
        let helpers: Vec<&Command> = cli
            .get_subcommands()
            .filter(|s| s.get_name().starts_with('+'))
            .collect();
        let resources: Vec<&Command> = cli
            .get_subcommands()
            .filter(|s| !s.get_name().starts_with('+'))
            .collect();
        let product_name = product_name_from_title("Test API");
        let md = render_service_skill(
            entry.aliases[0],
            entry,
            &helpers,
            &resources,
            &product_name,
            &doc,
        );
        let fm = extract_frontmatter(&md);
        assert_block_style_sequences(fm);
        assert!(
            fm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
            "frontmatter should contain version matching CLI version"
        );
        assert!(
            fm.contains("bins:\n"),
            "frontmatter should contain 'bins:' on its own line"
        );
        assert!(
            fm.contains("- gwsr"),
            "frontmatter should contain '- gwsr' block entry"
        );
    }

    #[test]
    fn test_shared_skill_frontmatter_uses_block_sequences() {
        let tmp = tempfile::tempdir().unwrap();
        generate_shared_skill(tmp.path(), false).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("gwsr-shared/SKILL.md")).unwrap();
        let fm = extract_frontmatter(&content);
        assert_block_style_sequences(fm);
        assert!(
            fm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
            "shared skill frontmatter should contain version matching CLI version"
        );
        assert!(
            fm.contains("- gwsr"),
            "shared skill frontmatter should contain '- gwsr'"
        );
    }

    #[test]
    fn test_persona_skill_frontmatter_uses_block_sequences() {
        let persona = PersonaEntry {
            name: "test-persona".to_string(),
            title: "Test Persona".to_string(),
            description: "A test persona for unit tests.".to_string(),
            services: vec!["gmail".to_string(), "calendar".to_string()],
            workflows: vec![],
            instructions: vec!["Do this.".to_string()],
            tips: vec![],
        };
        let md = render_persona_skill(&persona);
        let fm = extract_frontmatter(&md);
        assert_block_style_sequences(fm);
        assert!(
            fm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
            "persona frontmatter should contain version matching CLI version"
        );
        assert!(
            fm.contains("- gwsr"),
            "persona frontmatter should contain '- gwsr'"
        );
        assert!(
            fm.contains("- gwsr-gmail"),
            "persona frontmatter should contain '- gwsr-gmail'"
        );
        assert!(
            fm.contains("- gwsr-calendar"),
            "persona frontmatter should contain '- gwsr-calendar'"
        );
    }

    #[test]
    fn test_recipe_skill_frontmatter_uses_block_sequences() {
        let recipe = RecipeEntry {
            name: "test-recipe".to_string(),
            title: "Test Recipe".to_string(),
            description: "A test recipe for unit tests.".to_string(),
            category: "testing".to_string(),
            services: vec!["drive".to_string(), "sheets".to_string()],
            steps: vec!["Step one.".to_string()],
            caution: None,
        };
        let md = render_recipe_skill(&recipe);
        let fm = extract_frontmatter(&md);
        assert_block_style_sequences(fm);
        assert!(
            fm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
            "recipe frontmatter should contain version matching CLI version"
        );
        assert!(
            fm.contains("- gwsr"),
            "recipe frontmatter should contain '- gwsr'"
        );
        assert!(
            fm.contains("- gwsr-drive"),
            "recipe frontmatter should contain '- gwsr-drive'"
        );
        assert!(
            fm.contains("- gwsr-sheets"),
            "recipe frontmatter should contain '- gwsr-sheets'"
        );
    }

    #[test]
    fn test_helper_skill_frontmatter_uses_block_sequences() {
        // Use a service known to have helpers, e.g., drive
        let entry = services::SERVICES
            .iter()
            .find(|s| s.api_name == "drive")
            .unwrap();

        let doc = crate::discovery::RestDescription {
            name: entry.api_name.to_string(),
            title: Some("Test API".to_string()),
            description: Some(entry.description.to_string()),
            ..Default::default()
        };
        let cli = crate::commands::build_cli(&doc);
        let helper = cli
            .get_subcommands()
            .find(|s| s.get_name().starts_with('+'))
            .expect("No helper command found for test");

        let product_name = product_name_from_title("Test API");
        let md = render_helper_skill(
            entry.aliases[0],
            helper.get_name(),
            helper,
            entry,
            &product_name,
        );
        let fm = extract_frontmatter(&md);
        assert_block_style_sequences(fm);
        assert!(
            fm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
            "helper frontmatter should contain version matching CLI version"
        );
        assert!(
            fm.contains("bins:\n"),
            "frontmatter should contain 'bins:' on its own line"
        );
        assert!(
            fm.contains("- gwsr"),
            "frontmatter should contain '- gwsr' block entry"
        );
    }
}
