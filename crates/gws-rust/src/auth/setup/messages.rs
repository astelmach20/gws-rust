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

//! User-facing guidance for `gwsr auth setup`.

use std::path::Path;

fn is_tos_precondition_error(out: &str) -> bool {
    let lower = out.to_ascii_lowercase();
    lower.contains("callers must accept terms of service")
        || (lower.contains("terms of service") && lower.contains("type: tos"))
        || (lower.contains("failed_precondition") && lower.contains("type: tos"))
}

fn is_invalid_project_id_error(out: &str) -> bool {
    let lower = out.to_ascii_lowercase();
    lower.contains("argument project_id: bad value")
        || lower.contains("project ids must be between 6 and 30 characters")
}

fn is_project_id_in_use_error(out: &str) -> bool {
    let lower = out.to_ascii_lowercase();
    lower.contains("already in use")
        || lower.contains("already exists")
        || lower.contains("already being used")
        || lower.contains("project ids are immutable")
}

/// Explain a `gcloud projects create` failure.
pub fn format_project_create_failure(project_id: &str, account: &str, out: &str) -> String {
    if is_tos_precondition_error(out) {
        let mut msg = format!(
            "Failed to create project '{project_id}' because the active gcloud account has not \
             accepted Google Cloud Terms of Service.\n\nFix:\n\
             1. Verify the active account: `gcloud auth list` and `gcloud config get-value account`\n\
             2. Sign in to https://console.cloud.google.com/ with that same account and accept \
             Terms of Service.\n\
             3. Retry `gwsr auth setup` (or `gcloud projects create {project_id}`).\n\n\
             If this is a Google Workspace-managed account, an org admin may need to enable \
             Google Cloud for the domain first."
        );
        if !account.trim().is_empty() {
            msg.push_str(&format!("\n\nActive account in this setup run: {account}"));
        }
        return msg;
    }
    if is_invalid_project_id_error(out) {
        return format!(
            "Failed to create project '{project_id}' because the project ID format is invalid.\n\n\
             Project IDs must:\n- be 6 to 30 characters\n- start with a lowercase letter\n\
             - use only lowercase letters, digits, or hyphens\n\nEnter a new project ID and retry."
        );
    }
    if is_project_id_in_use_error(out) {
        return format!(
            "Failed to create project '{project_id}' because the ID is already in use. Enter a \
             different unique project ID and retry."
        );
    }
    if let Some(primary) = out.lines().map(str::trim).find(|l| l.starts_with("ERROR:")) {
        return format!(
            "Failed to create project '{project_id}'.\n\n{primary}\n\nEnter a different project \
             ID and retry."
        );
    }
    let details = out.trim();
    if details.is_empty() {
        format!("Failed to create project '{project_id}'. Enter a different project ID and retry.")
    } else {
        format!("Failed to create project '{project_id}'.\n\ngcloud error:\n{details}")
    }
}

/// Manual OAuth-client instructions (used when setup cannot prompt).
pub fn manual_oauth_instructions(project_id: &str, client_path: &Path) -> String {
    let q = if project_id.is_empty() {
        String::new()
    } else {
        format!("?project={project_id}")
    };
    format!(
        "OAuth client creation requires a manual step in the Google Cloud Console.\n\n\
         1. Configure the OAuth consent screen (if not already done):\n   \
         https://console.cloud.google.com/apis/credentials/consent{q}\n   \
         -> User type: Internal for a Workspace organisation (no verification, no test-user \
         list), otherwise External\n\n\
         2. Create an OAuth client ID:\n   \
         https://console.cloud.google.com/apis/credentials{q}\n   \
         -> Create Credentials -> OAuth client ID -> Application type: Desktop app\n\n\
         3. Provide it to gwsr, either:\n   \
         a) download the JSON and save it as {} (then `chmod 600` it), or\n   \
         b) export GWSR_CLIENT_ID and GWSR_CLIENT_SECRET\n\n\
         4. Run `gwsr auth login`.\n\n\
         Desktop clients need no redirect URI: gwsr uses http://127.0.0.1 with a random port.",
        client_path.display()
    )
}

/// What to do when gcloud is not installed.
pub fn gcloud_missing_message(client_path: &Path) -> String {
    format!(
        "gcloud CLI not found. `gwsr auth setup` uses it to create a project and enable APIs. \
         Either install it (https://cloud.google.com/sdk/docs/install) and re-run setup, or set \
         up the OAuth client manually:\n\n{}",
        manual_oauth_instructions("", client_path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tos_guidance() {
        let msg = format_project_create_failure(
            "example-project-123456",
            "user@example.com",
            "Operation failed: 9: Callers must accept Terms of Service\n type: TOS",
        );
        assert!(msg.contains("has not accepted Google Cloud Terms of Service"));
        assert!(msg.contains("gcloud auth list"));
        assert!(msg.contains("user@example.com"));
    }

    #[test]
    fn invalid_id_guidance() {
        let msg = format_project_create_failure(
            "p",
            "",
            "ERROR: (gcloud.projects.create) argument PROJECT_ID: Bad value [bad]: Project IDs must be between 6 and 30 characters.",
        );
        assert!(msg.contains("project ID format is invalid"));
    }

    #[test]
    fn in_use_guidance() {
        for out in [
            "Project ID already in use",
            "Project IDs are immutable and can be set only during project creation.",
        ] {
            assert!(format_project_create_failure("p", "", out).contains("already in use"));
        }
    }

    #[test]
    fn generic_error_lines() {
        let msg = format_project_create_failure("p", "", "noise\nERROR: specific thing\nmore");
        assert!(msg.contains("ERROR: specific thing"));
        assert!(format_project_create_failure("p", "", "").contains("Enter a different"));
    }

    #[test]
    fn gcloud_missing_lists_manual_options() {
        let msg = gcloud_missing_message(Path::new("/cfg/client_secret.json"));
        assert!(msg.contains("https://cloud.google.com/sdk/docs/install"));
        assert!(msg.contains("Desktop app"));
        assert!(msg.contains("/cfg/client_secret.json"));
        assert!(msg.contains("GWSR_CLIENT_ID"));
    }
}
