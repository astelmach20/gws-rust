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

//! Clap definitions for every Gmail helper command, plus argument helpers.
//!
//! Naming conventions (shared with the other helpers):
//! * opaque IDs use `--<resource>-id` (`--message-id`, `--thread-id`, `--filter-id`);
//! * local outputs use `--output-dir`;
//! * destructive or (by policy) outbound actions accept `--yes`/`-y`;
//! * `--dry-run` and `--format` are global flags, never redefined here.

use super::prelude::*;
use crate::helpers::rest::confirm::with_yes;

/// Read a required string argument. Clap enforces presence; this turns an
/// impossible absence into an error instead of a panic.
pub(super) fn required_str(matches: &ArgMatches, name: &str) -> Result<String, GwsError> {
    matches
        .get_one::<String>(name)
        .cloned()
        .ok_or_else(|| GwsError::Validation(format!("--{name} is required")))
}

/// Read a typed argument that has a default value.
pub(super) fn value_or_default<T: Clone + Send + Sync + 'static>(
    matches: &ArgMatches,
    name: &str,
) -> Result<T, GwsError> {
    matches
        .get_one::<T>(name)
        .cloned()
        .ok_or_else(|| other_error(format!("--{name} has no value (missing default)")))
}

/// Parse an optional clap argument, trimming whitespace and treating
/// empty/whitespace-only values as None.
pub(super) fn parse_optional_trimmed(matches: &ArgMatches, name: &str) -> Option<String> {
    matches
        .get_one::<String>(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse an optional clap argument as a comma-separated mailbox list.
/// Returns `None` when the argument is absent, empty, or yields no valid addresses.
pub(super) fn parse_optional_mailboxes(matches: &ArgMatches, name: &str) -> Option<Vec<Mailbox>> {
    parse_optional_trimmed(matches, name)
        .map(|s| Mailbox::parse_list(&s))
        .filter(|v| !v.is_empty())
}

/// Collect a repeatable, comma-separable list argument (`--x a,b --x c`).
pub(super) fn list_values(matches: &ArgMatches, name: &str) -> Vec<String> {
    matches
        .get_many::<String>(name)
        .map(|vals| {
            vals.flat_map(|v| v.split(','))
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn message_id_arg(help: &'static str) -> Arg {
    Arg::new("message-id")
        .long("message-id")
        .help(help)
        .required(true)
        .value_name("ID")
}

/// `--message-id`/`--thread-id` (repeatable) for commands that act on a set of targets.
fn target_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("message-id")
            .long("message-id")
            .help("Message ID to act on (repeatable or comma-separated)")
            .action(ArgAction::Append)
            .value_name("ID"),
    )
    .arg(
        Arg::new("thread-id")
            .long("thread-id")
            .help("Thread ID or Gmail web URL to act on (repeatable or comma-separated)")
            .action(ArgAction::Append)
            .value_name("ID|URL"),
    )
    .group(
        clap::ArgGroup::new("target")
            .args(["message-id", "thread-id"])
            .required(true)
            .multiple(true),
    )
}

/// Arguments shared by all composing commands.
fn common_mail_args(cmd: Command) -> Command {
    with_yes(
        cmd.arg(
            Arg::new("attach")
                .short('a')
                .long("attach")
                .help("Attach a file (repeatable)")
                .action(ArgAction::Append)
                .value_name("PATH"),
        )
        .arg(
            Arg::new("cc")
                .long("cc")
                .help("CC email address(es), comma-separated")
                .value_name("EMAILS"),
        )
        .arg(
            Arg::new("bcc")
                .long("bcc")
                .help("BCC email address(es), comma-separated")
                .value_name("EMAILS"),
        )
        .arg(
            Arg::new("html")
                .long("html")
                .help("Treat --body as HTML (a plain-text alternative is generated)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("draft")
                .long("draft")
                .help("Save as a draft instead of sending")
                .action(ArgAction::SetTrue),
        ),
    )
}

fn from_arg() -> Arg {
    Arg::new("from")
        .long("from")
        .help("Send-as address to send from (must be configured in Gmail; omit for the default)")
        .value_name("EMAIL")
}

/// Arguments shared by +reply and +reply-all (everything except --remove).
fn common_reply_args(cmd: Command) -> Command {
    common_mail_args(
        cmd.arg(message_id_arg("Gmail message ID to reply to"))
            .arg(
                Arg::new("body")
                    .long("body")
                    .help("Reply body (plain text, or HTML with --html)")
                    .required(true)
                    .value_name("TEXT"),
            )
            .arg(from_arg())
            .arg(
                Arg::new("to")
                    .long("to")
                    .help("Additional To email address(es), comma-separated")
                    .value_name("EMAILS"),
            ),
    )
}

fn send_cmd() -> Command {
    common_mail_args(
        Command::new("+send")
            .about("[Helper] Send an email")
            .arg(
                Arg::new("to")
                    .long("to")
                    .help("Recipient email address(es), comma-separated")
                    .required(true)
                    .value_name("EMAILS"),
            )
            .arg(
                Arg::new("subject")
                    .long("subject")
                    .help("Email subject")
                    .required(true)
                    .value_name("SUBJECT"),
            )
            .arg(
                Arg::new("body")
                    .long("body")
                    .help("Email body (plain text, or HTML with --html)")
                    .required(true)
                    .value_name("TEXT"),
            )
            .arg(from_arg()),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
  gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --cc bob@example.com
  gwsr gmail +send --to alice@example.com --subject 'Hello' --body '<b>Bold</b> text' --html
  gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --from alias@example.com
  gwsr gmail +send --to alice@example.com --subject 'Report' --body 'See attached' -a report.pdf
  gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --draft

TIPS:
  Handles RFC 5322 formatting, MIME encoding, and base64 automatically.
  --html sends multipart/alternative with a generated plain-text part.
  Total attachment size limit: 25MB.
  Sends are never retried automatically: a timeout may still have delivered the message.
  With GWSR_REQUIRE_CONFIRM=1, sending requires --yes (drafts do not).",
    )
}

fn reply_cmd() -> Command {
    common_reply_args(
        Command::new("+reply")
            .about("[Helper] Reply to a message (handles threading automatically)"),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
  gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
  gwsr gmail +reply --message-id 18f1a2b3c4d --body '<b>Bold reply</b>' --html
  gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Draft reply' --draft

TIPS:
  Sets In-Reply-To, References, and threadId, and quotes the original message.
  With --html, inline images in the quoted message are preserved via cid: references.
  For reply-all, use +reply-all instead.",
    )
}

fn reply_all_cmd() -> Command {
    common_reply_args(
        Command::new("+reply-all").about(
            "[Helper] Reply to all recipients of a message (handles threading automatically)",
        ),
    )
    .arg(
        Arg::new("remove")
            .long("remove")
            .help("Exclude recipients from the reply (comma-separated emails)")
            .value_name("EMAILS"),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
  gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
  gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com

TIPS:
  Replies to the sender and all original To/CC recipients, excluding yourself.
  The command fails if no To recipient remains after exclusions and --to additions.",
    )
}

fn forward_cmd() -> Command {
    common_mail_args(
        Command::new("+forward")
            .about("[Helper] Forward a message to new recipients")
            .arg(message_id_arg("Gmail message ID to forward"))
            .arg(
                Arg::new("to")
                    .long("to")
                    .help("Recipient email address(es), comma-separated")
                    .required(true)
                    .value_name("EMAILS"),
            )
            .arg(from_arg())
            .arg(
                Arg::new("body")
                    .long("body")
                    .help("Note to include above the forwarded message (plain text, or HTML with --html)")
                    .value_name("TEXT"),
            )
            .arg(
                Arg::new("no-original-attachments")
                    .long("no-original-attachments")
                    .help("Do not include the original message's file attachments")
                    .action(ArgAction::SetTrue),
            ),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
  gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
  gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --no-original-attachments

TIPS:
  Original attachments are included by default (matching Gmail web).
  In plain-text mode, inline images are not included (matching Gmail web).
  Combined size of original and added attachments is limited to 25MB.",
    )
}

fn read_cmd() -> Command {
    Command::new("+read")
        .about("[Helper] Read a message and print its body and optionally headers")
        .arg(message_id_arg("Gmail message ID to read"))
        .arg(
            Arg::new("headers")
                .long("headers")
                .help("With --body-format text, print From/To/Cc/Subject/Date before the body")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("body-format")
                .long("body-format")
                .help("Print the message as a JSON object or as plain text")
                .value_parser(["json", "text"])
                .default_value("json")
                .value_name("FORMAT"),
        )
        .arg(
            Arg::new("html")
                .long("html")
                .help("With --body-format text, print the HTML body instead of plain text")
                .action(ArgAction::SetTrue),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +read --message-id 18f1a2b3c4d | jq -r '.body_text'
  gwsr gmail +read --message-id 18f1a2b3c4d --body-format text --headers

TIPS:
  Prints the parsed message as JSON by default; --body-format text prints the body.
  HTML-only messages are rendered to plain text automatically.",
        )
}

fn triage_cmd() -> Command {
    Command::new("+triage")
        .about("[Helper] Show an unread inbox summary (sender, subject, date)")
        .arg(
            Arg::new("max")
                .long("max")
                .help("Maximum number of messages to show")
                .value_parser(clap::value_parser!(u32).range(1..=500))
                .default_value("20")
                .value_name("N"),
        )
        .arg(
            Arg::new("query")
                .long("query")
                .help("Gmail search query")
                .default_value("is:unread")
                .value_name("QUERY"),
        )
        .arg(
            Arg::new("labels")
                .long("labels")
                .help("Include label IDs in the output")
                .action(ArgAction::SetTrue),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +triage
  gwsr gmail +triage --max 5 --query 'from:boss'
  gwsr gmail +triage --format table
  gwsr gmail +triage | jq -r '.messages[].subject'

TIPS:
  Read-only. Use +search for full metadata and paging.",
        )
}

fn search_cmd() -> Command {
    Command::new("+search")
        .about("[Helper] Search messages and return full metadata, with pagination")
        .arg(
            Arg::new("query")
                .long("query")
                .help("Gmail search query (same syntax as the Gmail search box)")
                .value_name("QUERY"),
        )
        .arg(
            Arg::new("max")
                .long("max")
                .help("Maximum number of messages to return across pages")
                .value_parser(clap::value_parser!(u32).range(1..=10_000))
                .default_value("25")
                .value_name("N"),
        )
        .arg(
            Arg::new("page-token")
                .long("page-token")
                .help("Resume from a nextPageToken returned by a previous search")
                .value_name("TOKEN"),
        )
        .arg(
            Arg::new("include-spam-trash")
                .long("include-spam-trash")
                .help("Include messages from Spam and Trash")
                .action(ArgAction::SetTrue),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +search --query 'from:alice has:attachment newer_than:7d'
  gwsr gmail +search --query 'label:receipts' --max 200 --format table
  gwsr gmail +search --query 'is:starred' --page-token TOKEN

TIPS:
  Output includes id, threadId, labelIds, snippet, date, from, to, cc, subject, sizeEstimate.
  When more results exist than --max, the output includes nextPageToken.",
        )
}

fn label_cmd() -> Command {
    target_args(
        Command::new("+label")
            .about("[Helper] Add or remove labels on messages or threads")
            .arg(
                Arg::new("add")
                    .long("add")
                    .help("Label names or IDs to add (repeatable or comma-separated)")
                    .action(ArgAction::Append)
                    .value_name("LABEL"),
            )
            .arg(
                Arg::new("remove")
                    .long("remove")
                    .help("Label names or IDs to remove (repeatable or comma-separated)")
                    .action(ArgAction::Append)
                    .value_name("LABEL"),
            )
            .group(
                clap::ArgGroup::new("change")
                    .args(["add", "remove"])
                    .required(true)
                    .multiple(true),
            ),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +label --message-id 18f1a2b3c4d --add Receipts
  gwsr gmail +label --message-id ID1,ID2 --add Important --remove UNREAD
  gwsr gmail +label --thread-id 'https://mail.google.com/mail/u/0/#inbox/FMfcgz...' --add Follow-up

TIPS:
  Labels are matched by ID first, then by name (case-insensitive). Unknown labels are an error.",
    )
}

fn archive_cmd() -> Command {
    target_args(
        Command::new("+archive").about("[Helper] Archive messages or threads (remove from Inbox)"),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +archive --message-id 18f1a2b3c4d
  gwsr gmail +archive --thread-id 18f1a2b3c4d,18f1a2b3c4e",
    )
}

fn trash_cmd() -> Command {
    target_args(Command::new("+trash").about("[Helper] Move messages or threads to Trash"))
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +trash --message-id 18f1a2b3c4d
  gwsr gmail +trash --thread-id 18f1a2b3c4d

TIPS:
  Trashed items can be restored from Gmail's Trash for 30 days.",
        )
}

fn filter_cmd() -> Command {
    let create = Command::new("create")
        .about("Create a filter")
        .arg(
            Arg::new("from")
                .long("from")
                .help("Match sender")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .help("Match recipient")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("subject")
                .long("subject")
                .help("Match subject")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("query")
                .long("query")
                .help("Match a Gmail search query")
                .value_name("QUERY"),
        )
        .arg(
            Arg::new("exclude")
                .long("exclude")
                .help("Exclude messages matching this query")
                .value_name("QUERY"),
        )
        .arg(
            Arg::new("has-attachment")
                .long("has-attachment")
                .help("Match only messages with attachments")
                .action(ArgAction::SetTrue),
        )
        .group(
            clap::ArgGroup::new("criteria")
                .args([
                    "from",
                    "to",
                    "subject",
                    "query",
                    "exclude",
                    "has-attachment",
                ])
                .required(true)
                .multiple(true),
        )
        .arg(
            Arg::new("add-label")
                .long("add-label")
                .help("Label names or IDs to apply (repeatable or comma-separated)")
                .action(ArgAction::Append)
                .value_name("LABEL"),
        )
        .arg(
            Arg::new("remove-label")
                .long("remove-label")
                .help("Label names or IDs to remove (repeatable or comma-separated)")
                .action(ArgAction::Append)
                .value_name("LABEL"),
        )
        .arg(
            Arg::new("archive")
                .long("archive")
                .help("Skip the Inbox")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("mark-read")
                .long("mark-read")
                .help("Mark as read")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("star")
                .long("star")
                .help("Star the message")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("important")
                .long("important")
                .help("Mark as important")
                .action(ArgAction::SetTrue)
                .conflicts_with("never-important"),
        )
        .arg(
            Arg::new("never-important")
                .long("never-important")
                .help("Never mark as important")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("trash")
                .long("trash")
                .help("Move to Trash")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("never-spam")
                .long("never-spam")
                .help("Never send to Spam")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("forward")
                .long("forward")
                .help("Forward to this verified forwarding address")
                .value_name("EMAIL"),
        )
        .group(
            clap::ArgGroup::new("action")
                .args([
                    "add-label",
                    "remove-label",
                    "archive",
                    "mark-read",
                    "star",
                    "important",
                    "never-important",
                    "trash",
                    "never-spam",
                    "forward",
                ])
                .required(true)
                .multiple(true),
        );
    let delete = with_yes(
        Command::new("delete").about("Delete a filter").arg(
            Arg::new("filter-id")
                .long("filter-id")
                .help("ID of the filter to delete")
                .required(true)
                .value_name("ID"),
        ),
    );
    Command::new("+filter")
        .about("[Helper] List, create, or delete Gmail filters")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("List filters"))
        .subcommand(create)
        .subcommand(delete)
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +filter list
  gwsr gmail +filter create --from newsletter@example.com --add-label Newsletters --archive
  gwsr gmail +filter create --query 'subject:(invoice OR receipt)' --add-label Receipts
  gwsr gmail +filter delete --filter-id ANe1Bmj... --yes

TIPS:
  Requires the gmail.settings.basic scope (gwsr auth login -s gmail).
  Label names must already exist. Deleting a filter always requires --yes.",
        )
}

fn unsubscribe_cmd() -> Command {
    with_yes(
        Command::new("+unsubscribe")
            .about("[Helper] Unsubscribe from a mailing list via RFC 8058 one-click (List-Unsubscribe)")
            .arg(message_id_arg("ID of a message from the mailing list")),
    )
    .after_help(
        "\
EXAMPLES:
  gwsr gmail +unsubscribe --message-id 18f1a2b3c4d
  gwsr gmail +unsubscribe --message-id 18f1a2b3c4d --dry-run

TIPS:
  One-click unsubscribe is performed only when the message advertises
  List-Unsubscribe-Post: List-Unsubscribe=One-Click with an https URL and
  Gmail verified its DKIM signature. Otherwise the available unsubscribe
  links are printed and nothing is sent.",
    )
}

fn resolve_url_cmd() -> Command {
    Command::new("+resolve-url")
        .about("[Helper] Resolve a Gmail web URL (or its FMfcg... token) to an API thread or message ID")
        .arg(
            Arg::new("url")
                .long("url")
                .help("Gmail web URL, or the ID token at the end of it")
                .required(true)
                .value_name("URL"),
        )
        .arg(
            Arg::new("no-verify")
                .long("no-verify")
                .help("Decode offline only; do not confirm the ID exists via the API")
                .action(ArgAction::SetTrue),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +resolve-url --url 'https://mail.google.com/mail/u/0/#inbox/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW'
  gwsr gmail +resolve-url --url FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW --no-verify

TIPS:
  --thread-id on +label/+archive/+trash also accepts Gmail web URLs.",
        )
}

fn attachments_cmd() -> Command {
    Command::new("+attachments")
        .about("[Helper] Download a message's attachments as decoded files")
        .arg(message_id_arg(
            "Gmail message ID whose attachments to download",
        ))
        .arg(
            Arg::new("output-dir")
                .long("output-dir")
                .help("Directory to write files into (relative to the current directory)")
                .default_value(".")
                .value_name("DIR"),
        )
        .arg(
            Arg::new("include-inline")
                .long("include-inline")
                .help("Also save inline images")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("overwrite")
                .long("overwrite")
                .help("Overwrite existing files instead of failing")
                .action(ArgAction::SetTrue),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +attachments --message-id 18f1a2b3c4d
  gwsr gmail +attachments --message-id 18f1a2b3c4d --output-dir ./downloads --include-inline

TIPS:
  Filenames come from the sender and are sanitized; duplicates get a numeric suffix.
  Existing files are never overwritten unless --overwrite is given.",
        )
}

fn watch_cmd() -> Command {
    Command::new("+watch")
        .about("[Helper] Watch for new emails and stream them as NDJSON")
        .arg(
            Arg::new("project")
                .long("project")
                .help("GCP project ID for Pub/Sub resources (or set GWSR_PROJECT_ID)")
                .value_name("PROJECT"),
        )
        .arg(
            Arg::new("subscription")
                .long("subscription")
                .help("Existing Pub/Sub subscription name (skip setup)")
                .value_name("NAME"),
        )
        .arg(
            Arg::new("topic")
                .long("topic")
                .help("Existing Pub/Sub topic with Gmail publish permission already granted")
                .value_name("TOPIC"),
        )
        .arg(
            Arg::new("label-ids")
                .long("label-ids")
                .help("Comma-separated Gmail label IDs to watch (e.g., INBOX,UNREAD)")
                .value_name("LABELS"),
        )
        .arg(
            Arg::new("max-messages")
                .long("max-messages")
                .help("Maximum Pub/Sub messages per pull")
                .value_parser(clap::value_parser!(u32).range(1..=1000))
                .default_value("10")
                .value_name("N"),
        )
        .arg(
            Arg::new("poll-interval")
                .long("poll-interval")
                .help("Seconds between pulls")
                .value_parser(clap::value_parser!(u64).range(1..=3600))
                .default_value("5")
                .value_name("SECS"),
        )
        .arg(
            Arg::new("max-failures")
                .long("max-failures")
                .help("Consecutive transient failures (429/5xx/network) tolerated before exiting")
                .value_parser(clap::value_parser!(u32).range(1..))
                .default_value("10")
                .value_name("N"),
        )
        .arg(
            Arg::new("msg-format")
                .long("msg-format")
                .help("Gmail message format")
                .value_parser(["full", "metadata", "minimal", "raw"])
                .default_value("full")
                .value_name("FORMAT"),
        )
        .arg(
            Arg::new("once")
                .long("once")
                .help("Pull once and exit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("cleanup")
                .long("cleanup")
                .help("Delete created Pub/Sub resources on exit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("output-dir")
                .long("output-dir")
                .help("Write each message to a separate JSON file in this directory")
                .value_name("DIR"),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr gmail +watch --project my-gcp-project
  gwsr gmail +watch --project my-project --label-ids INBOX --once
  gwsr gmail +watch --subscription projects/p/subscriptions/my-sub
  gwsr gmail +watch --project my-project --cleanup --output-dir ./emails

TIPS:
  stdout carries one JSON message per line. Transient errors are retried with
  backoff and reported as single-line JSON on stderr; the watcher exits after
  --max-failures consecutive failures. Delivery is at-least-once.
  Gmail watch expires after 7 days; re-run to renew. Press Ctrl-C to stop.",
        )
}

/// Register every Gmail helper subcommand.
pub(super) fn inject_commands(cmd: Command) -> Command {
    cmd.subcommand(send_cmd())
        .subcommand(reply_cmd())
        .subcommand(reply_all_cmd())
        .subcommand(forward_cmd())
        .subcommand(read_cmd())
        .subcommand(triage_cmd())
        .subcommand(search_cmd())
        .subcommand(label_cmd())
        .subcommand(archive_cmd())
        .subcommand(trash_cmd())
        .subcommand(filter_cmd())
        .subcommand(unsubscribe_cmd())
        .subcommand(resolve_url_cmd())
        .subcommand(attachments_cmd())
        .subcommand(watch_cmd())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::helper_matches;

    fn gmail_root() -> Command {
        crate::commands::build_cli(&crate::discovery::RestDescription {
            name: "gmail".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn test_inject_commands() {
        let cmd = inject_commands(Command::new("test"));
        let names: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        for expected in [
            "+send",
            "+reply",
            "+reply-all",
            "+forward",
            "+read",
            "+triage",
            "+search",
            "+label",
            "+archive",
            "+trash",
            "+filter",
            "+unsubscribe",
            "+resolve-url",
            "+attachments",
            "+watch",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn test_command_tree_is_valid() {
        gmail_root().debug_assert();
    }

    #[test]
    fn test_every_helper_about_is_capitalized_and_tagged() {
        for sub in inject_commands(Command::new("t")).get_subcommands() {
            let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
            let rest = about.strip_prefix("[Helper] ").unwrap_or_else(|| {
                panic!("{} about must start with [Helper]: {about}", sub.get_name())
            });
            assert!(
                rest.chars().next().is_some_and(char::is_uppercase),
                "{} about must be capitalized: {about}",
                sub.get_name()
            );
        }
    }

    #[test]
    fn test_help_does_not_duplicate_defaults() {
        fn check(cmd: &Command) {
            for arg in cmd.get_arguments() {
                let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
                assert!(
                    !help.contains("(default"),
                    "--{} help repeats its default: {help}",
                    arg.get_id()
                );
            }
            for sub in cmd.get_subcommands() {
                check(sub);
            }
        }
        check(&inject_commands(Command::new("t")));
    }

    #[test]
    fn test_message_id_is_the_only_spelling() {
        let root = gmail_root();
        for name in [
            "+read",
            "+reply",
            "+reply-all",
            "+forward",
            "+unsubscribe",
            "+attachments",
        ] {
            let err = root
                .clone()
                .try_get_matches_from(["gwsr", name, "--id", "x"])
                .unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "{name}"
            );
        }
        let m = helper_matches(&["+read", "--message-id", "abc"]);
        assert_eq!(m.get_one::<String>("message-id").unwrap(), "abc");
    }

    #[test]
    fn test_read_body_format_replaces_format() {
        let m = helper_matches(&["+read", "--message-id", "x", "--body-format", "text"]);
        assert_eq!(m.get_one::<String>("body-format").unwrap(), "text");
        // JSON by default; --format is the global output format, not a +read option.
        let m = helper_matches(&["+read", "--message-id", "x", "--format", "yaml"]);
        assert_eq!(m.get_one::<String>("body-format").unwrap(), "json");
    }

    #[test]
    fn test_label_requires_target_and_change() {
        let root = gmail_root();
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+label", "--add", "X"])
                .is_err()
        );
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+label", "--message-id", "m"])
                .is_err()
        );
        assert!(
            root.try_get_matches_from(["gwsr", "+label", "--message-id", "m", "--add", "X"])
                .is_ok()
        );
    }

    #[test]
    fn test_list_values_splits_and_trims() {
        let m = helper_matches(&["+archive", "--message-id", "a, b", "--message-id", "c,"]);
        assert_eq!(list_values(&m, "message-id"), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_numeric_flags_are_validated() {
        let root = gmail_root();
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+triage", "--max", "abc"])
                .is_err()
        );
        assert!(
            root.try_get_matches_from(["gwsr", "+watch", "--poll-interval", "0"])
                .is_err()
        );
    }

    #[test]
    fn test_parse_optional_trimmed() {
        let cmd = Command::new("test")
            .arg(Arg::new("flag").long("flag"))
            .arg(Arg::new("empty").long("empty"))
            .arg(Arg::new("ws").long("ws"));

        // Present, non-empty value
        let matches = cmd
            .clone()
            .try_get_matches_from(["test", "--flag", "value"])
            .unwrap();
        assert_eq!(
            parse_optional_trimmed(&matches, "flag"),
            Some("value".to_string())
        );

        // Absent argument
        let matches = cmd.clone().try_get_matches_from(["test"]).unwrap();
        assert!(parse_optional_trimmed(&matches, "flag").is_none());

        // Whitespace-only becomes None
        let matches = cmd
            .clone()
            .try_get_matches_from(["test", "--ws", "  "])
            .unwrap();
        assert!(parse_optional_trimmed(&matches, "ws").is_none());

        // Empty string becomes None
        let matches = cmd.try_get_matches_from(["test", "--empty", ""]).unwrap();
        assert!(parse_optional_trimmed(&matches, "empty").is_none());
    }
}
