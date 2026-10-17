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
    text.chars().filter(|&c| terminal_safe(c)).collect()
}

/// Whether [`sanitize_for_terminal`] keeps `c`.
fn terminal_safe(c: char) -> bool {
    if c == '\n' || c == '\t' {
        return true;
    }
    if c.is_control() {
        return false;
    }
    !is_dangerous_unicode(c)
}

/// Zero-width non-joiner and joiner (U+200C, U+200D): invisible, but part of
/// ordinary text (Persian and Indic scripts, emoji sequences such as 👨‍👩‍👧).
/// They neither move the cursor nor reorder text.
pub(crate) fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | '\u{200D}')
}

/// [`sanitize_for_terminal`] for natural-language API text printed as data
/// (table and CSV cells, raw `--jq` strings): the zero-width joiners such
/// text uses ([`is_joiner`]) are kept.
pub(crate) fn sanitize_text_for_terminal(text: &str) -> String {
    text.chars()
        .filter(|&c| is_joiner(c) || terminal_safe(c))
        .collect()
}

/// Escape, as `\uXXXX`, every character in serialized JSON that a terminal
/// could act on but `serde_json` writes raw: C1 controls (U+0080–U+009F,
/// such as U+009B, an 8-bit CSI), DEL, and the dangerous Unicode of
/// [`is_dangerous_unicode`] (bidi overrides and isolates, zero-width
/// characters, line separators). `serde_json` already escapes C0 controls
/// inside strings.
///
/// The JSON value is unchanged: these characters only occur inside strings
/// (JSON's structure is ASCII), and every one is in the Basic Multilingual
/// Plane, so a single `\uXXXX` escape decodes back to it. JSON whitespace
/// (`\n`, `\r`, `\t`) between tokens is left alone.
pub(crate) fn escape_json_for_terminal(json: String) -> String {
    fn needs_escape(c: char) -> bool {
        (c.is_control() && !matches!(c, '\n' | '\r' | '\t')) || is_dangerous_unicode(c)
    }
    if !json.chars().any(needs_escape) {
        return json;
    }
    let mut out = String::with_capacity(json.len() + 16);
    for c in json.chars() {
        if needs_escape(c) {
            out.push_str(&format!("\\u{:04x}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
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

/// Returns true when stderr is connected to an interactive terminal that
/// renders ANSI color codes and `NO_COLOR` is not set.
pub(crate) fn stderr_supports_color() -> bool {
    color_enabled(stderr_is_terminal(), no_color(), console_renders_ansi)
}

/// Whether `NO_COLOR` asks for no color.
pub(crate) fn no_color() -> bool {
    no_color_requested(std::env::var_os("NO_COLOR").as_deref())
}

/// <https://no-color.org>: `NO_COLOR` disables color when it is present
/// and not an empty string, whatever its value. An empty `NO_COLOR=` is
/// treated as unset.
fn no_color_requested(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Whether to color: a terminal, no `NO_COLOR`, and a console that turns
/// escape sequences into color (asked last, since asking may change the
/// console's mode).
pub(crate) fn color_enabled(
    terminal: bool,
    no_color: bool,
    renders_ansi: impl FnOnce() -> bool,
) -> bool {
    terminal && !no_color && renders_ansi()
}

/// A Windows console prints escape sequences as literal text (`←[1;31m`)
/// unless virtual terminal processing is enabled, which the classic console
/// host does not do for programs it starts. crossterm enables it once and
/// reports whether the console can show ANSI (true for Windows Terminal,
/// for consoles that accepted the mode, and for terminals such as mintty
/// that set `TERM`).
#[cfg(windows)]
pub(crate) fn console_renders_ansi() -> bool {
    crossterm::ansi_support::supports_ansi()
}

/// Unix terminals render ANSI sequences.
#[cfg(not(windows))]
pub(crate) fn console_renders_ansi() -> bool {
    true
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

/// Write `text`, ended by a newline, to stdout and flush.
///
/// Every command writes its results through this function instead of
/// `println!`, which panics when the reader closes the pipe. An empty `text`
/// writes nothing; a newline is added unless `text` already ends with one
/// (so `"\n"` is one empty line: see [`terminated_line`]). A closed pipe is
/// recorded (see [`stdout_closed`]) and returned as an error so callers stop
/// producing output; `main` turns it into a clean exit.
pub(crate) fn emit(text: &str) -> Result<(), GwsError> {
    if text.is_empty() {
        return Ok(());
    }
    write_line(&mut std::io::stdout().lock(), text).map_err(stdout_error)
}

/// The bytes [`emit`] writes for `text`: nothing for an empty text,
/// otherwise `text` ended by exactly the newline it lacks.
pub(crate) fn terminated_line(text: &str) -> std::borrow::Cow<'_, str> {
    if text.is_empty() || text.ends_with('\n') {
        std::borrow::Cow::Borrowed(text)
    } else {
        std::borrow::Cow::Owned(format!("{text}\n"))
    }
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
    w.write_all(terminated_line(text).as_bytes())?;
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
    fn json_escape_covers_c1_controls_and_dangerous_unicode() {
        let value = serde_json::json!({
            "m": "a\u{9b}31mb\u{85}c\u{7f}d\u{202E}e\u{2066}f\u{200B}g\u{2028}h\u{1b}i",
            "ok": "日本語 café\tαβγ",
        });
        let raw = serde_json::to_string_pretty(&value).unwrap();
        let escaped = escape_json_for_terminal(raw);
        assert!(
            !escaped
                .chars()
                .any(|c| (c.is_control() && c != '\n') || is_dangerous_unicode(c)),
            "{escaped:?}"
        );
        for esc in [
            "\\u009b", "\\u0085", "\\u007f", "\\u202e", "\\u2066", "\\u200b", "\\u2028", "\\u001b",
        ] {
            assert!(escaped.contains(esc), "{esc} missing from {escaped}");
        }
        // Lossless: the escaped text parses back to the same value.
        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed, value);
        assert!(escaped.contains("日本語 café\\tαβγ"), "{escaped}");
    }

    #[test]
    fn text_sanitizer_keeps_joiners_only() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let persian = "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0645}";
        assert_eq!(sanitize_text_for_terminal(family), family);
        assert_eq!(sanitize_text_for_terminal(persian), persian);
        assert_eq!(
            sanitize_text_for_terminal("a\u{200B}b\u{202E}c\u{1b}[2Jd\u{9b}e\u{200D}f"),
            "abc[2Jde\u{200D}f"
        );
        // The general sanitizer still removes them.
        assert_eq!(sanitize_for_terminal(family), "\u{1F468}\u{1F469}\u{1F467}");
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

    /// A console that cannot render ANSI (a Windows console without virtual
    /// terminal processing) gets plain text, not `←[1;31merror←[0m`.
    #[test]
    fn color_needs_a_console_that_renders_ansi() {
        assert!(color_enabled(true, false, || true));
        assert!(!color_enabled(true, false, || false));
        assert!(!color_enabled(false, false, || true));
        assert!(!color_enabled(true, true, || true));
        // The console is not asked (or switched into VT mode) when color is
        // off anyway.
        assert!(!color_enabled(false, false, || unreachable!()));
        assert!(!color_enabled(true, true, || unreachable!()));
    }

    /// `NO_COLOR=` (empty) is unset, per no-color.org; any other value,
    /// `0` and `false` included, disables color.
    #[test]
    fn empty_no_color_is_unset() {
        use std::ffi::OsStr;
        assert!(!no_color_requested(None));
        assert!(!no_color_requested(Some(OsStr::new(""))));
        for value in ["1", "0", "false", " "] {
            assert!(no_color_requested(Some(OsStr::new(value))), "{value:?}");
        }
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

    /// Text that already ends its line (a `--jq` result, an empty line) is
    /// written as is.
    #[test]
    fn write_line_does_not_double_a_trailing_newline() {
        for (text, written) in [("\n", "\n"), ("a\n", "a\n"), ("a\n\n", "a\n\n")] {
            let mut buf = Vec::new();
            write_line(&mut buf, text).unwrap();
            assert_eq!(String::from_utf8(buf).unwrap(), written, "{text:?}");
        }
    }

    #[test]
    fn write_line_reports_broken_pipe() {
        let err = write_line(&mut ClosedPipe, "abc").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }
}
