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

//! Shared output helpers for terminal sanitization, coloring, and stderr
//! messaging.
//!
//! Every function that prints untrusted content to the terminal should use
//! these helpers to prevent escape-sequence injection, Unicode spoofing,
//! and to respect `NO_COLOR` / non-TTY environments.

// Import dangerous-char detection from the library crate.
pub(crate) use gws_rust_core::validate::is_dangerous_unicode;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::GwsError;

// ── Sanitization ──────────────────────────────────────────────────────

/// Strip dangerous characters from untrusted text before printing to the
/// terminal.  Removes ASCII control characters (except `\n` and `\t`,
/// which are preserved for readability) and dangerous Unicode characters
/// (bidi overrides, zero-width chars, line/paragraph separators).
pub(crate) fn sanitize_for_terminal(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            if c == '\n' || c == '\t' {
                return true;
            }
            if c.is_control() {
                return false;
            }
            !is_dangerous_unicode(c)
        })
        .collect()
}

// ── Terminal detection ────────────────────────────────────────────────

/// True when stdout is an interactive terminal.
pub(crate) fn stdout_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// True when stderr is an interactive terminal.
pub(crate) fn stderr_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

// ── Color ─────────────────────────────────────────────────────────────

/// Returns true when stderr is connected to an interactive terminal and
/// `NO_COLOR` is not set, meaning ANSI color codes will be visible.
pub(crate) fn stderr_supports_color() -> bool {
    stderr_is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Wrap `text` in ANSI bold + the given color code, resetting afterwards.
/// Returns the plain text unchanged when stderr is not a TTY or `NO_COLOR`
/// is set.
pub(crate) fn colorize(text: &str, ansi_color: &str) -> String {
    paint(text, ansi_color, stderr_supports_color())
}

fn paint(text: &str, ansi_color: &str, enabled: bool) -> String {
    if enabled && ansi_color.chars().all(|c| c.is_ascii_digit()) {
        format!("\x1b[1;{ansi_color}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ── Stdout ────────────────────────────────────────────────────────────

/// Set once stdout's reader has gone away (EPIPE). `main` checks it to exit
/// quietly with status 0 instead of reporting an error, the conventional
/// behaviour for `gwsr ... | head`.
static STDOUT_CLOSED: AtomicBool = AtomicBool::new(false);

/// True once a write to stdout failed with `BrokenPipe`.
pub(crate) fn stdout_closed() -> bool {
    STDOUT_CLOSED.load(Ordering::Relaxed)
}

/// Write `text` followed by a newline to stdout and flush.
///
/// Every command writes its results through this function instead of
/// `println!`, which panics when the reader closes the pipe. An empty `text`
/// writes nothing. A closed pipe is recorded (see [`stdout_closed`]) and
/// returned as an error so callers stop producing output; `main` turns it into
/// a clean exit.
pub(crate) fn emit(text: &str) -> Result<(), GwsError> {
    if text.is_empty() {
        return Ok(());
    }
    write_line(&mut std::io::stdout().lock(), text).map_err(stdout_error)
}

/// Convert a failed stdout write into an error, recording a closed pipe
/// (see [`stdout_closed`]). Used by every writer of stdout, including the
/// executor's raw-byte stream.
pub(crate) fn stdout_error(e: std::io::Error) -> GwsError {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        STDOUT_CLOSED.store(true, Ordering::Relaxed);
    }
    GwsError::other(anyhow::Error::new(e).context("failed to write to stdout"))
}

fn write_line(w: &mut dyn Write, text: &str) -> std::io::Result<()> {
    if STDOUT_CLOSED.load(Ordering::Relaxed) {
        return Err(std::io::ErrorKind::BrokenPipe.into());
    }
    w.write_all(text.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

// ── Stderr helpers ────────────────────────────────────────────────────

/// Print a line to stderr. Unlike `eprintln!`, a closed or failing stderr
/// never panics: there is nowhere left to report the failure, so it is
/// deliberately dropped.
pub(crate) fn eprint_line(text: &str) {
    let mut err = std::io::stderr().lock();
    if writeln!(err, "{text}").is_err() {
        // stderr is gone; nothing more can be reported.
    }
}

/// Print `text` to stderr without a newline and flush (prompts, progress
/// bars). Failures are dropped for the same reason as [`eprint_line`].
pub(crate) fn eprint_text(text: &str) {
    let mut err = std::io::stderr().lock();
    if err
        .write_all(text.as_bytes())
        .and_then(|()| err.flush())
        .is_err()
    {
        // stderr is gone; nothing more can be reported.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_for_terminal ─────────────────────────────────────

    #[test]
    fn sanitize_strips_ansi_escape_sequences() {
        let input = "normal \x1b[31mred text\x1b[0m end";
        let sanitized = sanitize_for_terminal(input);
        assert_eq!(sanitized, "normal [31mred text[0m end");
        assert!(!sanitized.contains('\x1b'));
    }

    #[test]
    fn sanitize_preserves_newlines_and_tabs() {
        let input = "line1\nline2\ttab";
        assert_eq!(sanitize_for_terminal(input), "line1\nline2\ttab");
    }

    #[test]
    fn sanitize_strips_bell_and_backspace() {
        let input = "hello\x07bell\x08backspace";
        assert_eq!(sanitize_for_terminal(input), "hellobellbackspace");
    }

    #[test]
    fn sanitize_strips_carriage_return() {
        let input = "real\rfake";
        assert_eq!(sanitize_for_terminal(input), "realfake");
    }

    #[test]
    fn sanitize_strips_bidi_overrides() {
        let input = "hello\u{202E}dlrow";
        assert_eq!(sanitize_for_terminal(input), "hellodlrow");
    }

    #[test]
    fn sanitize_strips_zero_width_chars() {
        assert_eq!(sanitize_for_terminal("foo\u{200B}bar"), "foobar");
        assert_eq!(sanitize_for_terminal("foo\u{FEFF}bar"), "foobar");
    }

    #[test]
    fn sanitize_strips_line_separators() {
        assert_eq!(sanitize_for_terminal("line1\u{2028}line2"), "line1line2");
        assert_eq!(sanitize_for_terminal("para1\u{2029}para2"), "para1para2");
    }

    #[test]
    fn sanitize_strips_directional_isolates() {
        assert_eq!(sanitize_for_terminal("a\u{2066}b\u{2069}c"), "abc");
    }

    #[test]
    fn sanitize_preserves_normal_unicode() {
        assert_eq!(sanitize_for_terminal("日本語 café αβγ"), "日本語 café αβγ");
    }

    // ── colorize ──────────────────────────────────────────────────

    #[test]
    fn paint_respects_enabled_flag() {
        assert_eq!(paint("hello", "31", false), "hello");
        assert_eq!(paint("hello", "31", true), "\x1b[1;31mhello\x1b[0m");
        // Non-numeric colour codes are never interpolated.
        assert_eq!(paint("hello", "31m;evil", true), "hello");
    }

    struct ClosedPipe;
    impl Write for ClosedPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_line_appends_newline() {
        let mut buf = Vec::new();
        write_line(&mut buf, "abc").unwrap();
        assert_eq!(buf, b"abc\n");
    }

    #[test]
    fn write_line_reports_broken_pipe() {
        let err = write_line(&mut ClosedPipe, "abc").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }
}
