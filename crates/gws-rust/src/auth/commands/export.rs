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

//! `gwsr auth export`.
//!
//! * default: prints the profile's credentials with every secret masked; if
//!   the stored data cannot be parsed it fails instead of printing anything
//! * `--unmasked --output FILE`: writes the real credentials to a *new* 0600
//!   file, after an interactive confirmation (or
//!   `--yes-i-understand-this-exposes-secrets` when stdin is not a terminal).
//!   Secrets are never printed to stdout, so they cannot leak into an agent's
//!   context or a CI log by accident.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{auth_err, print_json};
use crate::auth::credentials::AuthEnv;
use crate::auth::keystore::Purpose;
use crate::error::GwsError;

const SECRET_FIELDS: &[&str] = &["client_secret", "refresh_token", "private_key"];

pub(super) async fn handle(m: &clap::ArgMatches) -> Result<(), GwsError> {
    let env = AuthEnv::from_process().map_err(|e| auth_err(format!("{e:#}")))?;
    let plaintext = tokio::task::spawn_blocking(move || read_profile_plaintext(&env))
        .await
        .map_err(|e| auth_err(format!("export task failed: {e}")))??;

    if crate::args::flag(m, "unmasked")? {
        let output = crate::args::value::<String>(m, "output")?
            .map(PathBuf::from)
            .ok_or_else(|| GwsError::Validation("--unmasked requires --output FILE".into()))?;
        let confirmed = crate::args::flag(m, "confirm")?;
        let stdin_tty = std::io::stdin().is_terminal();
        confirm(confirmed, stdin_tty, &output, prompt_yes)?;
        write_new_private_file(&output, plaintext.as_bytes())?;
        print_json(&json!({
            "status": "success",
            "file": output.display().to_string(),
            "message": "Unmasked credentials written (mode 0600). Anyone who can read this file \
                        can act as you; delete it when done.",
        }))
    } else {
        print_json(&masked(&plaintext)?)
    }
}

fn read_profile_plaintext(env: &AuthEnv) -> Result<Zeroizing<String>, GwsError> {
    let path = &env.paths.credentials;
    if !path.exists() {
        return Err(GwsError::Auth(format!(
            "profile '{}' has no stored credentials to export (run `gwsr auth login`)",
            env.profile.name
        )));
    }
    let data = std::fs::read(path)
        .map_err(|e| auth_err(format!("cannot read '{}': {e}", path.display())))?;
    let what = format!("the credentials of profile '{}'", env.profile.name);
    let keystore = env.keystore().map_err(auth_err)?;
    let pt = keystore
        .decrypt(Purpose::Credentials, &data, &what)
        .map_err(auth_err)?;
    let text = std::str::from_utf8(&pt)
        .map_err(|_| auth_err(format!("{what} decrypted to invalid UTF-8")))?;
    Ok(Zeroizing::new(text.to_string()))
}

/// Mask secrets; fail (without printing) if the data is not a JSON object.
pub(super) fn masked(plaintext: &str) -> Result<Value, GwsError> {
    let mut value: Value = serde_json::from_str(plaintext).map_err(|_| {
        GwsError::Auth(
            "the stored credentials are not valid JSON; refusing to print them. Run `gwsr auth \
             logout` and `gwsr auth login` to replace them"
                .into(),
        )
    })?;
    let obj = value.as_object_mut().ok_or_else(|| {
        GwsError::Auth(
            "the stored credentials are not a JSON object; refusing to print them".into(),
        )
    })?;
    for key in SECRET_FIELDS {
        if let Some(v) = obj.get_mut(*key) {
            *v = match v.as_str() {
                Some(s) => json!(mask_secret(s)),
                None => json!("***"),
            };
        }
    }
    Ok(value)
}

/// Show only the first and last 4 characters of long secrets.
pub(super) fn mask_secret(s: &str) -> String {
    let count = s.chars().count();
    if count > 16 {
        let head: String = s.chars().take(4).collect();
        let tail: String = s.chars().skip(count - 4).collect();
        format!("{head}...{tail}")
    } else {
        "***".to_string()
    }
}

/// Require confirmation before writing secrets.
pub(super) fn confirm(
    flag: bool,
    stdin_tty: bool,
    output: &Path,
    prompt: impl FnOnce(&Path) -> Result<bool, GwsError>,
) -> Result<(), GwsError> {
    if flag {
        return Ok(());
    }
    if !stdin_tty {
        return Err(GwsError::Validation(
            "refusing to export unmasked credentials non-interactively; pass \
             --yes-i-understand-this-exposes-secrets if this is intended"
                .into(),
        ));
    }
    if prompt(output)? {
        Ok(())
    } else {
        Err(GwsError::Validation("export cancelled".into()))
    }
}

fn prompt_yes(output: &Path) -> Result<bool, GwsError> {
    crate::output::eprint_text(&format!(
        "This writes your refresh token and client secret in plain text to '{}'.\n\
         Type 'yes' to continue: ",
        output.display()
    ));
    std::io::stderr()
        .flush()
        .map_err(|e| GwsError::Validation(format!("cannot write prompt: {e}")))?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| GwsError::Validation(format!("cannot read confirmation: {e}")))?;
    Ok(line.trim() == "yes")
}

/// Create `path` exclusively with mode 0600 (never overwrite, never follow a
/// pre-existing symlink).
pub(super) fn write_new_private_file(path: &Path, data: &[u8]) -> Result<(), GwsError> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| {
        GwsError::Validation(format!(
            "cannot create '{}': {e} (the file must not already exist)",
            path.display()
        ))
    })?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(|e| GwsError::Validation(format!("cannot write '{}': {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_all_secret_fields() {
        let v = masked(
            r#"{"type":"authorized_user","client_id":"cid","client_secret":"GOCSPX-abcdefghijklmnop","refresh_token":"1//0abcdefghijklmnopqrstuvwxyz"}"#,
        )
        .unwrap();
        let text = v.to_string();
        assert!(!text.contains("abcdefghijklmnop"), "{text}");
        assert_eq!(v["client_id"], "cid");
        assert_eq!(v["refresh_token"], "1//0...wxyz");
    }

    /// SEC-08: the masked path must fail closed on unparseable data.
    #[test]
    fn unparseable_data_is_an_error_not_printed() {
        let err = masked("refresh_token=1//secret").unwrap_err().to_string();
        assert!(!err.contains("secret"), "{err}");
        assert!(masked("[1,2]").is_err());
    }

    #[test]
    fn mask_secret_lengths() {
        assert_eq!(mask_secret("short"), "***");
        assert_eq!(mask_secret("ééééééééééééééééé"), "éééé...éééé");
    }

    /// SEC-08: non-interactive unmasked export needs the explicit flag.
    #[test]
    fn confirmation_rules() {
        let out = Path::new("x");
        let never = |_: &Path| -> Result<bool, GwsError> { panic!("must not prompt") };
        assert!(confirm(false, false, out, never).is_err());
        confirm(true, false, out, never).unwrap();
        confirm(false, true, out, |_| Ok(true)).unwrap();
        assert!(confirm(false, true, out, |_| Ok(false)).is_err());
    }

    #[test]
    fn output_file_is_new_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.json");
        write_new_private_file(&path, b"{}").unwrap();
        assert!(
            write_new_private_file(&path, b"{}").is_err(),
            "no overwrite"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let link = dir.path().join("link.json");
            std::os::unix::fs::symlink(dir.path().join("target.json"), &link).unwrap();
            assert!(
                write_new_private_file(&link, b"{}").is_err(),
                "no symlink follow"
            );
        }
    }
}
