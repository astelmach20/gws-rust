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

//! Strict parser for Model Armor template resource names (SEC-05).
//!
//! The template's location segment becomes part of the request hostname
//! (`modelarmor.{location}.rep.googleapis.com`), so it must never be
//! interpolated without a grammar check.

use reqwest::Url;

use super::endpoint::validate_api_base_with;
use crate::error::GwsError;

/// A validated `projects/{project}/locations/{location}/templates/{id}` name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelArmorTemplate {
    project: String,
    location: String,
    template_id: String,
}

/// Project ID (`[a-z][a-z0-9-]{4,28}[a-z0-9]`) or project number (digits).
fn is_valid_project(p: &str) -> bool {
    let bytes = p.as_bytes();
    let is_id = (6..=30).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-');
    let is_number = (1..=20).contains(&bytes.len()) && bytes.iter().all(u8::is_ascii_digit);
    is_id || is_number
}

/// Location: a DNS label `[a-z0-9-]+`, 1..=63 chars, alphanumeric at both ends.
fn is_valid_location(l: &str) -> bool {
    let bytes = l.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Template ID: `[A-Za-z0-9_-]{1,63}`.
fn is_valid_template_id(t: &str) -> bool {
    (1..=63).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl ModelArmorTemplate {
    /// Parse `projects/{project}/locations/{location}/templates/{template_id}`.
    pub fn parse(name: &str) -> Result<Self, GwsError> {
        let invalid = |why: &str| {
            GwsError::Validation(format!(
                "Invalid Model Armor template '{}': {why}. Expected \
                 'projects/PROJECT/locations/LOCATION/templates/TEMPLATE_ID'",
                name.escape_debug()
            ))
        };
        let parts: Vec<&str> = name.split('/').collect();
        let [
            projects,
            project,
            locations,
            location,
            templates,
            template_id,
        ] = parts.as_slice()
        else {
            return Err(invalid("wrong number of path segments"));
        };
        if *projects != "projects" || *locations != "locations" || *templates != "templates" {
            return Err(invalid("unexpected collection names"));
        }
        if !is_valid_project(project) {
            return Err(invalid("invalid project ID"));
        }
        if !is_valid_location(location) {
            return Err(invalid("invalid location"));
        }
        if !is_valid_template_id(template_id) {
            return Err(invalid("invalid template ID"));
        }
        Ok(Self {
            project: (*project).to_string(),
            location: (*location).to_string(),
            template_id: (*template_id).to_string(),
        })
    }

    /// Build from separate components, validating each.
    pub fn from_parts(project: &str, location: &str, template_id: &str) -> Result<Self, GwsError> {
        Self::parse(&format!(
            "projects/{project}/locations/{location}/templates/{template_id}"
        ))
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn location(&self) -> &str {
        &self.location
    }

    pub fn template_id(&self) -> &str {
        &self.template_id
    }

    /// The full resource name.
    pub fn name(&self) -> String {
        format!(
            "projects/{}/locations/{}/templates/{}",
            self.project, self.location, self.template_id
        )
    }

    /// The regional API base, `https://modelarmor.{location}.rep.googleapis.com/v1/`,
    /// checked against the trusted-host rules.
    pub fn api_base(&self) -> Result<Url, GwsError> {
        validate_api_base_with(
            &format!(
                "https://modelarmor.{}.rep.googleapis.com/v1/",
                self.location
            ),
            None,
        )
    }
}

impl std::fmt::Display for ModelArmorTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_templates() {
        let t = ModelArmorTemplate::parse(
            "projects/my-proj-1/locations/us-central1/templates/tmpl_A-1",
        )
        .unwrap();
        assert_eq!(t.project(), "my-proj-1");
        assert_eq!(t.location(), "us-central1");
        assert_eq!(t.template_id(), "tmpl_A-1");
        assert_eq!(
            t.name(),
            "projects/my-proj-1/locations/us-central1/templates/tmpl_A-1"
        );
        assert!(ModelArmorTemplate::parse("projects/123456789/locations/eu/templates/t").is_ok());
        assert_eq!(
            ModelArmorTemplate::from_parts("my-proj-1", "us", "t")
                .unwrap()
                .to_string(),
            "projects/my-proj-1/locations/us/templates/t"
        );
    }

    #[test]
    fn rejects_invalid_templates() {
        for bad in [
            "",
            "projects/my-proj-1/locations/us-central1/templates",
            "projects/my-proj-1/locations/us-central1/templates/t/extra",
            "project/my-proj-1/locations/us/templates/t",
            "projects/my-proj-1/locations//templates/t",
            "projects/my-proj-1/locations/evil.com@x/templates/t",
            "projects/my-proj-1/locations/evil.com#/templates/t",
            "projects/my-proj-1/locations/us:443/templates/t",
            "projects/my-proj-1/locations/us%2e/templates/t",
            "projects/my-proj-1/locations/us central/templates/t",
            "projects/my-proj-1/locations/us\\x/templates/t",
            "projects/my-proj-1/locations/US/templates/t",
            "projects/my-proj-1/locations/-us/templates/t",
            "projects/my-proj-1/locations/us/templates/t@x",
            "projects/My-Proj/locations/us/templates/t",
            "projects/p/locations/us/templates/t",
            "projects/my-proj-1-/locations/us/templates/t",
        ] {
            assert!(ModelArmorTemplate::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn api_base_host_is_always_regional_googleapis() {
        for loc in ["us-central1", "europe-west4", "us"] {
            let t = ModelArmorTemplate::from_parts("my-proj-1", loc, "t").unwrap();
            let url = t.api_base().unwrap();
            assert_eq!(url.scheme(), "https");
            let host = url.host_str().unwrap();
            assert!(host.ends_with(".rep.googleapis.com"), "{host}");
            assert_eq!(host, format!("modelarmor.{loc}.rep.googleapis.com"));
        }
    }
}
