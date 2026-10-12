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

//! Thin async wrapper around the `gcloud` CLI. Every failure is returned
//! with gcloud's own error output; nothing is silently ignored.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use secrecy::SecretString;
use tokio::process::Command;

const LIST_PROJECTS_TIMEOUT: Duration = Duration::from_secs(20);
const ENABLE_CONCURRENCY: usize = 5;

/// The gcloud executable name for this platform (`gcloud.cmd` on Windows).
pub fn default_bin() -> &'static str {
    if cfg!(windows) {
        "gcloud.cmd"
    } else {
        "gcloud"
    }
}

/// Result of enabling APIs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnableResult {
    pub enabled: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// A gcloud installation.
#[derive(Debug, Clone)]
pub struct Gcloud {
    bin: PathBuf,
}

impl Default for Gcloud {
    fn default() -> Self {
        Self {
            bin: PathBuf::from(default_bin()),
        }
    }
}

fn combined_output(out: &std::process::Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.trim().is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(stdout.trim());
    }
    s
}

fn value_or_none(stdout: &[u8]) -> Option<String> {
    let v = String::from_utf8_lossy(stdout).trim().to_string();
    if v.is_empty() || v == "(unset)" {
        None
    } else {
        Some(v)
    }
}

impl Gcloud {
    /// Use a specific executable (tests).
    #[cfg(test)]
    pub fn with_bin(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.env("CLOUDSDK_CORE_DISABLE_PROMPTS", "1")
            .kill_on_drop(true);
        cmd
    }

    async fn output(&self, args: &[&str]) -> anyhow::Result<std::process::Output> {
        self.cmd()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .with_context(|| format!("failed to run `{} {}`", self.bin.display(), args.join(" ")))
    }

    async fn checked(&self, args: &[&str]) -> anyhow::Result<std::process::Output> {
        let out = self.output(args).await?;
        if !out.status.success() {
            anyhow::bail!(
                "`gcloud {}` failed ({}): {}",
                args.join(" "),
                out.status,
                crate::output::sanitize_for_terminal(&combined_output(&out))
            );
        }
        Ok(out)
    }

    /// Whether gcloud can be executed. `Ok(false)` means "not installed".
    ///
    /// # Errors
    ///
    /// gcloud exists but could not be run for another reason.
    pub async fn is_installed(&self) -> anyhow::Result<bool> {
        match self
            .cmd()
            .arg("version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) => Ok(status.success()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).with_context(|| format!("cannot run '{}'", self.bin.display())),
        }
    }

    /// Run `gcloud auth login` interactively (opens a browser).
    ///
    /// # Errors
    ///
    /// gcloud cannot run or the login fails.
    pub async fn auth_login(&self) -> anyhow::Result<()> {
        let status = self
            .cmd()
            .args(["auth", "login"])
            .status()
            .await
            .context("failed to run `gcloud auth login`")?;
        if !status.success() {
            anyhow::bail!("`gcloud auth login` failed ({status})");
        }
        Ok(())
    }

    /// The active account, if any.
    ///
    /// # Errors
    ///
    /// gcloud fails.
    pub async fn account(&self) -> anyhow::Result<Option<String>> {
        let out = self.checked(&["config", "get-value", "account"]).await?;
        Ok(value_or_none(&out.stdout))
    }

    /// All credentialed accounts as `(email, is_active)`.
    ///
    /// # Errors
    ///
    /// gcloud fails.
    pub async fn accounts(&self) -> anyhow::Result<Vec<(String, bool)>> {
        let out = self
            .checked(&["auth", "list", "--format=value(account,status)"])
            .await?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let account = parts.next().filter(|a| !a.is_empty())?;
                let active = parts.next().is_some_and(|s| s.contains("ACTIVE"));
                Some((account.to_string(), active))
            })
            .collect())
    }

    /// # Errors
    ///
    /// gcloud fails.
    pub async fn set_account(&self, account: &str) -> anyhow::Result<()> {
        self.checked(&["config", "set", "account", account])
            .await
            .map(|_| ())
    }

    /// The configured project, if any.
    ///
    /// # Errors
    ///
    /// gcloud fails.
    pub async fn project(&self) -> anyhow::Result<Option<String>> {
        let out = self.checked(&["config", "get-value", "project"]).await?;
        Ok(value_or_none(&out.stdout))
    }

    /// # Errors
    ///
    /// gcloud fails.
    pub async fn set_project(&self, project: &str) -> anyhow::Result<()> {
        self.checked(&["config", "set", "project", project])
            .await
            .map(|_| ())
    }

    /// Accessible projects as `(id, name)`, with a timeout (some managed
    /// devices make this call hang).
    ///
    /// # Errors
    ///
    /// gcloud fails or times out.
    pub async fn projects(&self) -> anyhow::Result<Vec<(String, String)>> {
        let out = tokio::time::timeout(
            LIST_PROJECTS_TIMEOUT,
            self.checked(&[
                "projects",
                "list",
                "--format=value(projectId,name)",
                "--sort-by=projectId",
            ]),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "timed out after {}s listing projects",
                LIST_PROJECTS_TIMEOUT.as_secs()
            )
        })??;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let id = parts.next().filter(|i| !i.is_empty())?;
                Some((id.to_string(), parts.next().unwrap_or("").to_string()))
            })
            .collect())
    }

    /// Create a project. On failure returns gcloud's combined output.
    ///
    /// # Errors
    ///
    /// `Err(output)` when gcloud reports a failure; `anyhow` errors when it
    /// cannot run at all are mapped into the same string.
    pub async fn create_project(&self, project_id: &str) -> Result<(), String> {
        let out = self
            .output(&["projects", "create", project_id])
            .await
            .map_err(|e| format!("{e:#}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(combined_output(&out))
        }
    }

    /// An access token for the active gcloud account.
    ///
    /// # Errors
    ///
    /// gcloud fails (e.g. not logged in).
    pub async fn access_token(&self) -> anyhow::Result<SecretString> {
        let out = self
            .checked(&["auth", "print-access-token"])
            .await
            .context("cannot get a gcloud access token; run `gcloud auth login` first")?;
        let token =
            zeroize::Zeroizing::new(String::from_utf8_lossy(&out.stdout).trim().to_string());
        if token.is_empty() {
            anyhow::bail!("gcloud returned an empty access token");
        }
        Ok(SecretString::from(token.to_string()))
    }

    /// Service IDs enabled for `project`.
    ///
    /// # Errors
    ///
    /// gcloud fails or returns unparseable JSON.
    pub async fn enabled_apis(&self, project: &str) -> anyhow::Result<Vec<String>> {
        let out = self
            .checked(&[
                "services",
                "list",
                "--enabled",
                "--project",
                project,
                "--format=json",
            ])
            .await?;
        let services: Vec<serde_json::Value> =
            serde_json::from_slice(&out.stdout).context("gcloud returned invalid JSON")?;
        Ok(services
            .iter()
            .filter_map(|s| s.pointer("/config/name").and_then(|n| n.as_str()))
            .map(str::to_string)
            .collect())
    }

    /// Enable `api_ids` for `project`, in parallel; one failure does not stop
    /// the others. Already-enabled APIs are skipped.
    ///
    /// # Errors
    ///
    /// Listing the already-enabled APIs fails.
    pub async fn enable_apis(
        &self,
        project: &str,
        api_ids: &[String],
    ) -> anyhow::Result<EnableResult> {
        use futures_util::stream::StreamExt;

        let mut result = EnableResult::default();
        if api_ids.is_empty() {
            return Ok(result);
        }
        let already = self.enabled_apis(project).await?;
        let (skip, todo): (Vec<String>, Vec<String>) =
            api_ids.iter().cloned().partition(|id| already.contains(id));
        result.skipped = skip;

        let outcomes: Vec<(String, anyhow::Result<std::process::Output>)> =
            futures_util::stream::iter(todo)
                .map(|id| async move {
                    let out = self
                        .output(&["services", "enable", &id, "--project", project])
                        .await;
                    (id, out)
                })
                .buffer_unordered(ENABLE_CONCURRENCY)
                .collect()
                .await;
        for (id, out) in outcomes {
            match out {
                Ok(o) if o.status.success() => result.enabled.push(id),
                Ok(o) => {
                    let msg = combined_output(&o);
                    let msg = if msg.is_empty() {
                        format!("gcloud services enable failed ({})", o.status)
                    } else {
                        msg
                    };
                    result.failed.push((id, msg));
                }
                Err(e) => result.failed.push((id, format!("{e:#}"))),
            }
        }
        result.enabled.sort();
        result.failed.sort();
        Ok(result)
    }
}

/// Enabled APIs for `project` using the default gcloud.
///
/// # Errors
///
/// See [`Gcloud::enabled_apis`].
pub async fn get_enabled_apis(project: &str) -> anyhow::Result<Vec<String>> {
    Gcloud::default().enabled_apis(project).await
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake gcloud: a shell script with canned behaviour per subcommand.
    ///
    /// The script is written by a child `sh`, not by this process. A file
    /// this process opened for writing would be inherited by any child that
    /// another test thread forks at the same moment, and while that child
    /// held the descriptor, running the script would fail with ETXTBSY
    /// ("Text file busy").
    fn fake_gcloud(dir: &std::path::Path, script: &str) -> Gcloud {
        let path = dir.join("gcloud");
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", r#"printf '%s\n' "$1" > "$2" && chmod 755 "$2""#, "sh"])
            .arg(format!("#!/bin/sh\n{script}"))
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "writing the fake gcloud failed: {status}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        Gcloud::with_bin(path)
    }

    #[tokio::test]
    async fn missing_binary_is_not_installed() {
        let g = Gcloud::with_bin("/nonexistent/gcloud-xyz");
        assert!(!g.is_installed().await.unwrap());
    }

    #[tokio::test]
    async fn config_values_and_unset() {
        let dir = tempfile::tempdir().unwrap();
        let g = fake_gcloud(
            dir.path(),
            r#"case "$3" in account) echo me@example.com;; project) echo '(unset)';; esac"#,
        );
        assert_eq!(
            g.account().await.unwrap().as_deref(),
            Some("me@example.com")
        );
        assert_eq!(g.project().await.unwrap(), None);
    }

    #[tokio::test]
    async fn failures_carry_gcloud_output() {
        let dir = tempfile::tempdir().unwrap();
        let g = fake_gcloud(dir.path(), "echo 'ERROR: boom' >&2; exit 1");
        let err = g.account().await.unwrap_err().to_string();
        assert!(err.contains("ERROR: boom"), "{err}");
        assert_eq!(g.create_project("p").await.unwrap_err(), "ERROR: boom");
    }

    #[tokio::test]
    async fn enable_apis_reports_each_result() {
        let dir = tempfile::tempdir().unwrap();
        let g = fake_gcloud(
            dir.path(),
            r#"if [ "$1 $2" = "services list" ]; then echo '[{"config":{"name":"drive.googleapis.com"}}]'; exit 0; fi
if [ "$3" = "vault.googleapis.com" ]; then echo 'PERMISSION_DENIED' >&2; exit 1; fi
exit 0"#,
        );
        let ids: Vec<String> = [
            "drive.googleapis.com",
            "gmail.googleapis.com",
            "vault.googleapis.com",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = g.enable_apis("proj", &ids).await.unwrap();
        assert_eq!(r.skipped, vec!["drive.googleapis.com"]);
        assert_eq!(r.enabled, vec!["gmail.googleapis.com"]);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].0, "vault.googleapis.com");
        assert!(r.failed[0].1.contains("PERMISSION_DENIED"));

        assert_eq!(
            g.enable_apis("proj", &[]).await.unwrap(),
            EnableResult::default()
        );
    }

    #[tokio::test]
    async fn lists_accounts_and_projects() {
        let dir = tempfile::tempdir().unwrap();
        let g = fake_gcloud(
            dir.path(),
            r#"case "$1" in auth) printf 'a@x.com\t*ACTIVE*\nb@x.com\t\n';; projects) printf 'p1\tOne\np2\n';; esac"#,
        );
        assert_eq!(
            g.accounts().await.unwrap(),
            vec![
                ("a@x.com".to_string(), true),
                ("b@x.com".to_string(), false)
            ]
        );
        assert_eq!(
            g.projects().await.unwrap(),
            vec![
                ("p1".to_string(), "One".to_string()),
                ("p2".to_string(), String::new())
            ]
        );
    }

    #[test]
    fn bin_name() {
        assert_eq!(default_bin(), "gcloud");
    }
}
