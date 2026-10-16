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

/// The platform's own launchers, in order of preference, without the URL.
fn platform_openers() -> &'static [&'static [&'static str]] {
    if cfg!(target_os = "macos") {
        &[&["/usr/bin/open"]]
    } else if cfg!(windows) {
        // Not `cmd /c start`, which would interpret `&` and `%` in the URL.
        &[&["rundll32.exe", "url.dll,FileProtocolHandler"]]
    } else {
        // `wslview` opens the Windows browser from WSL.
        &[&["xdg-open"], &["wslview"]]
    }
}

/// The commands a `$BROWSER` value names: a `:`-separated list whose entries
/// are split into words the way a shell would (quotes, backslashes) but are
/// never run by one. Every `%s` in a word becomes the URL; an entry without
/// `%s` gets the URL as its last argument. Either way the URL stays a single
/// argument. An entry that cannot be split is skipped with a warning.
fn browser_var_commands(value: &str, url: &str) -> Vec<Vec<String>> {
    value
        .split(':')
        .filter_map(|entry| match shell_words::split(entry) {
            Ok(words) if words.is_empty() => None,
            Ok(words) => Some(words),
            Err(e) => {
                tracing::warn!("ignoring the $BROWSER entry {entry:?}: {e}");
                None
            }
        })
        .map(|mut words| {
            if words.iter().any(|w| w.contains("%s")) {
                words.iter().map(|w| w.replace("%s", url)).collect()
            } else {
                words.push(url.to_string());
                words
            }
        })
        .collect()
}

/// The commands that open `url`, in order of preference: on Unix those that
/// `$BROWSER` (`browser_var`) names, then the platform's launchers.
fn browser_commands(browser_var: Option<&OsStr>, url: &str) -> Vec<std::process::Command> {
    let mut argvs = Vec::new();
    if cfg!(unix) {
        match browser_var.map(OsStr::to_str) {
            Some(Some(value)) => argvs.extend(browser_var_commands(value, url)),
            Some(None) => tracing::warn!("ignoring $BROWSER: it is not valid UTF-8"),
            None => {}
        }
    }
    argvs.extend(platform_openers().iter().map(|argv| {
        let mut argv: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
        argv.push(url.to_string());
        argv
    }));
    commands_for(&argvs)
}

/// A detached [`command`] for each argv.
fn commands_for(argvs: &[Vec<String>]) -> Vec<std::process::Command> {
    argvs
        .iter()
        .filter_map(|argv| argv.split_first())
        .map(|(program, args)| {
            let mut cmd = command(program);
            cmd.args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            cmd
        })
        .collect()
}

/// Open `url` in the default browser without waiting for it. On Unix the
/// commands in `$BROWSER` are tried first, then the platform's launcher; the
/// first one that starts wins. A launcher that later exits unsuccessfully is
/// reported as a warning.
///
/// # Errors
///
/// No browser launcher is installed, or none could be started.
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    launch_first(browser_commands(
        std::env::var_os("BROWSER").as_deref(),
        url,
    ))
}

/// Start the first of `cmds` that can be started, without waiting for it.
fn launch_first(cmds: Vec<std::process::Command>) -> std::io::Result<()> {
    let mut not_found = None;
    let mut failed = None;
    for mut cmd in cmds {
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
                tracing::debug!("{program} is not installed");
                not_found.get_or_insert_with(|| {
                    std::io::Error::new(e.kind(), format!("{program} is not installed"))
                });
            }
            Err(e) => {
                tracing::warn!("cannot run {program}: {e}");
                failed.get_or_insert_with(|| {
                    std::io::Error::new(e.kind(), format!("cannot run {program}: {e}"))
                });
            }
        }
    }
    Err(failed.or(not_found).unwrap_or_else(|| {
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

    fn argv(cmd: &std::process::Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
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
        for browser in [None, Some(OsStr::new("my-browser --new-window"))] {
            let cmds = browser_commands(browser, url);
            assert!(!cmds.is_empty());
            for cmd in &cmds {
                assert!(removed(cmd, "GWSR_TOKEN"), "{cmd:?}");
                assert!(removed(cmd, "GWSR_CLIENT_SECRET"), "{cmd:?}");
                // The URL is one argument, never parsed by a shell.
                assert_eq!(cmd.get_args().last(), Some(OsStr::new(url)), "{cmd:?}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn browser_variable_commands_come_first() {
        let url = "https://accounts.example.com/auth?a=1&b=$(id);x";
        let cmds = browser_commands(
            Some(OsStr::new(
                "firefox -P 'my profile' --url=%s::w3m:\"/opt/My Browser/run\"",
            )),
            url,
        );
        let argvs: Vec<Vec<String>> = cmds.iter().map(argv).collect();
        assert_eq!(
            argvs[0],
            ["firefox", "-P", "my profile", &format!("--url={url}")]
        );
        assert_eq!(argvs[1], ["w3m", url]);
        assert_eq!(argvs[2], ["/opt/My Browser/run", url]);
        // Then the platform's launchers.
        assert_eq!(argvs.len(), 3 + platform_openers().len());
        assert_eq!(argvs[3][0], platform_openers()[0][0]);
    }

    #[cfg(unix)]
    #[test]
    fn unsplittable_or_empty_browser_entries_are_skipped() {
        let cmds = browser_commands(Some(OsStr::new("  :'unterminated:ok")), "u");
        let argvs: Vec<Vec<String>> = cmds.iter().map(argv).collect();
        assert_eq!(argvs[0], ["ok", "u"]);
        assert_eq!(argvs.len(), 1 + platform_openers().len());
        let cmds = browser_commands(Some(OsStr::new("")), "u");
        assert_eq!(cmds.len(), platform_openers().len());
    }

    /// Writes an executable `/bin/sh` script from a child `sh`, so no
    /// descriptor of this (multi-threaded) process holds it open for writing
    /// when it runs (ETXTBSY).
    #[cfg(unix)]
    fn write_script(path: &std::path::Path, body: &str) {
        let status = command("/bin/sh")
            .args(["-c", r#"printf '%s\n' "$1" > "$2" && chmod 755 "$2""#, "sh"])
            .arg(format!("#!/bin/sh\n{body}"))
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// `$BROWSER` is honoured: the first of its commands that starts gets
    /// the URL as a single argument, with no shell in between.
    #[cfg(unix)]
    #[test]
    fn browser_variable_launches_the_first_command_that_starts() {
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("argv.txt");
        let browser = dir.path().join("fake browser");
        write_script(
            &browser,
            &format!(
                "printf '%s\\n' \"$@\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'",
                args.display()
            ),
        );
        let missing = dir.path().join("not-installed");
        let value = format!(
            "{}:{} --flag %s",
            missing.display(),
            shell_words::quote(&browser.to_string_lossy())
        );
        let url = "https://accounts.example.com/auth?a=1&b=2;echo hi $HOME";
        // Only `$BROWSER`'s commands, so a failure here can never fall back
        // to opening a real browser.
        launch_first(commands_for(&browser_var_commands(&value, url))).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !args.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the fake browser did not run"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let seen = std::fs::read_to_string(&args).unwrap();
        assert_eq!(seen, format!("--flag\n{url}\n"));
    }

    /// When no command can start, the error names the missing program.
    #[cfg(unix)]
    #[test]
    fn launching_only_missing_programs_fails() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-installed");
        let value = missing.to_string_lossy().into_owned();
        let err = launch_first(commands_for(&browser_var_commands(&value, "u"))).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("not-installed"), "{err}");
    }
}
