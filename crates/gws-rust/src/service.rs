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

//! Loading a service's Discovery document and building its command tree.

use crate::discovery::RestDescription;
use crate::error::GwsError;

/// API name of the synthetic cross-service `workflow` service, which has no
/// Discovery document.
pub const WORKFLOW_API: &str = "workflow";

/// The document for a synthetic service, if `api_name` is one.
pub fn synthetic_document(api_name: &str) -> Option<RestDescription> {
    (api_name == WORKFLOW_API).then(|| RestDescription {
        name: WORKFLOW_API.to_string(),
        title: Some("Workflow".to_string()),
        description: Some("Cross-service productivity workflows".to_string()),
        ..Default::default()
    })
}

/// Fetch (or read from cache) the Discovery document for `api_name/version`.
pub async fn load_document(api_name: &str, version: &str) -> Result<RestDescription, GwsError> {
    if let Some(doc) = synthetic_document(api_name) {
        return Ok(doc);
    }
    crate::discovery::fetch_discovery_document(api_name, version).await
}

/// Build the clap command for a service, named as the user invoked it.
pub fn build_command(alias: &str, doc: &RestDescription) -> clap::Command {
    crate::commands::build_cli(doc)
        .name(alias.to_string())
        .bin_name(format!("gwsr {alias}"))
        .disable_version_flag(true)
}
