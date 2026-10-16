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

//! Child processes. Every program `gwsr` runs (`gcloud`, the browser opener)
//! is built by [`command`] or [`async_command`], which withhold `gwsr`'s own
//! environment variables, secrets such as `GWSR_TOKEN` and
//! `GWSR_CLIENT_SECRET` included (see [`crate::env::withheld_from_children`]).
//! `clippy.toml` disallows `Command::new` everywhere else.

use std::ffi::OsStr;
use std::process::Stdio;

/// A [`std::process::Command`] for `program` that does not inherit any
/// `GWSR_*` variable. The rest of the environment is passed on.
#[allow(clippy::disallowed_methods)] // the one place that may call it
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    for name in crate::env::withheld_from_children() {
        cmd.env_remove(name);
    }
    cmd
}

/// [`command`] for tokio.
pub fn async_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    tokio::process::Command::from(command(program))
}

/// The commands that open `url` in the default browser, in order of
/// preference; the next one is tried only when a program is not installed.
fn browser_commands(url: &str) -> Vec<std::process::Command> {
    let openers: &[&[&str]] = if cfg!(target_os = "macos") {
        &[&["/usr/bin/open"]]
    } else if cfg!(windows) {
        // Not `cmd /c start`, which would interpret `&` and `%` in the URL.
        &[&["rundll32.exe", "url.dll,FileProtocolHandler"]]
    } else {
        // `wslview` opens the Windows browser from WSL.
        &[&["xdg-open"], &["wslview"]]
    };
    openers
        .iter()
        .filter_map(|argv| argv.split_first())
        .map(|(program, args)| {
            let mut cmd = command(program);
            cmd.args(args)
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            cmd
        })
        .collect()
}

/// Open `url` in the default browser without waiting for it. A launcher that
/// later exits unsuccessfully is reported as a warning.
///
/// # Errors
///
/// No browser launcher is installed, or one could not be started.
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    let mut not_found = None;
    for mut cmd in browser_commands(url) {
        let program = cmd.get_program().to_string_lossy().into_owned();
        match cmd.spawn() {
            Ok(mut child) => {
                // Reap the launcher so it does not linger as a zombie, and
                // say so if it failed.
                std::thread::spawn(move || match child.wait() {
                    Ok(status) if status.success() => {}
                    Ok(status) => tracing::warn!(
                        "{program} could not open a browser ({status}); open the URL above manually"
                    ),
                    Err(e) => tracing::warn!("cannot wait for {program}: {e}"),
                });
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                not_found = Some(std::io::Error::new(
                    e.kind(),
                    format!("{program} is not installed"),
                ));
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("cannot run {program}: {e}"),
                ));
            }
        }
    }
    Err(not_found.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no browser launcher for this platform",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn removed(cmd: &std::process::Command, name: &str) -> bool {
        cmd.get_envs()
            .any(|(k, v)| k == OsStr::new(name) && v.is_none())
    }

    #[test]
    fn commands_withhold_gwsr_variables() {
        let cmd = command("true");
        for name in [
            "GWSR_TOKEN",
            "GWSR_CLIENT_SECRET",
            "GWSR_DEFAULT_CLIENT_SECRET",
        ] {
            assert!(removed(&cmd, name), "{name}");
        }
        let cmd = async_command("true");
        assert!(removed(cmd.as_std(), "GWSR_TOKEN"));
    }

    /// The browser launcher, and so the browser it starts, never sees
    /// `GWSR_TOKEN` or `GWSR_CLIENT_SECRET`.
    #[test]
    fn browser_launchers_withhold_gwsr_variables() {
        let url = "https://accounts.example.com/o/oauth2/auth?a=1&b=%2F";
        let cmds = browser_commands(url);
        assert!(!cmds.is_empty());
        for cmd in &cmds {
            assert!(removed(cmd, "GWSR_TOKEN"), "{cmd:?}");
            assert!(removed(cmd, "GWSR_CLIENT_SECRET"), "{cmd:?}");
            // The URL is one argument, never parsed by a shell.
            assert_eq!(cmd.get_args().last(), Some(OsStr::new(url)), "{cmd:?}");
        }
    }
}
