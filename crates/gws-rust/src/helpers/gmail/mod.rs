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

//! Gmail helpers (`gwsr gmail +<helper>`).
//!
//! Module layout:
//! * [`cli`] — clap definitions for every helper command.
//! * [`api`] — typed Gmail REST calls (injectable base URLs for tests).
//! * [`address`], [`message`], [`html`], [`mime`] — pure parsing/formatting.
//! * [`attachments`], [`sender`], [`dispatch`] — shared building blocks.
//! * One module per command family: [`send`], [`reply`], [`forward`], [`read`],
//!   [`triage`], [`search`], [`labels`], [`filter`], [`unsubscribe`],
//!   [`resolve_url`], [`download`], [`watch`].

mod address;
mod api;
mod attachments;
mod cli;
mod dispatch;
mod download;
mod filter;
mod forward;
mod html;
mod labels;
mod message;
mod mime;
mod read;
mod reply;
mod resolve_url;
mod search;
mod send;
mod sender;
#[cfg(test)]
mod test_support;
mod triage;
mod unsubscribe;
mod watch;

use super::Helper;
use crate::error::GwsError;
use clap::{ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;

/// Items shared by every Gmail helper submodule (`use super::prelude::*`).
mod prelude {
    pub(super) use super::address::*;
    pub(super) use super::api::{GmailApi, TargetKind};
    pub(super) use super::attachments::*;
    pub(super) use super::html::*;
    pub(super) use super::message::*;
    pub(super) use super::mime::*;
    pub(super) use super::{GMAIL_READONLY_SCOPE, GMAIL_SCOPE, GMAIL_SETTINGS_SCOPE, PUBSUB_SCOPE};
    pub(super) use crate::error::GwsError;
    pub(super) use crate::output::sanitize_for_terminal;
    pub(super) use base64::Engine as _;
    pub(super) use clap::{Arg, ArgAction, ArgMatches, Command};
    pub(super) use mail_builder::headers::address::Address as MbAddress;
    pub(super) use serde::Serialize;
    pub(super) use serde_json::{Value, json};
}

/// Print a helper's (non-dry-run) result in `format`, after Model Armor
/// screening when `--sanitize` is configured. A block-mode match or failure
/// prints nothing and returns the error.
pub(super) async fn emit_screened(
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
    format: &crate::formatter::OutputFormat,
    value: serde_json::Value,
) -> Result<(), GwsError> {
    use crate::helpers::modelarmor::{require_pass, sanitize_value};
    let value = require_pass(sanitize_value(sanitize, value).await?)?;
    crate::output::emit(&crate::formatter::format_value(&value, format)?)
}

pub struct GmailHelper;

/// Read/write scope used for fetching messages and sending/drafting.
pub(super) const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
/// Read-only scope for helpers that never modify the mailbox.
pub(super) const GMAIL_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// Scope for managing filters (`users.settings.filters`).
pub(super) const GMAIL_SETTINGS_SCOPE: &str =
    "https://www.googleapis.com/auth/gmail.settings.basic";
pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";

impl Helper for GmailHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cli::inject_commands(cmd)
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            let Some((name, sub)) = matches.subcommand() else {
                return Ok(false);
            };
            match name {
                "+send" => send::handle_send(sub, sanitize_config).await?,
                "+reply" => reply::handle_reply(sub, false, sanitize_config).await?,
                "+reply-all" => reply::handle_reply(sub, true, sanitize_config).await?,
                "+forward" => forward::handle_forward(sub, sanitize_config).await?,
                "+read" => read::handle_read(sub, sanitize_config).await?,
                "+triage" => triage::handle_triage(sub, sanitize_config).await?,
                "+search" => search::handle_search(sub, sanitize_config).await?,
                "+label" => labels::handle_label(sub, sanitize_config).await?,
                "+archive" => labels::handle_archive(sub, sanitize_config).await?,
                "+trash" => labels::handle_trash(sub, sanitize_config).await?,
                "+filter" => filter::handle_filter(sub, sanitize_config).await?,
                "+unsubscribe" => unsubscribe::handle_unsubscribe(sub, sanitize_config).await?,
                "+resolve-url" => resolve_url::handle_resolve_url(sub, sanitize_config).await?,
                "+attachments" => download::handle_attachments(sub, sanitize_config).await?,
                "+watch" => watch::handle_watch(sub, sanitize_config).await?,
                _ => return Ok(false),
            }
            Ok(true)
        })
    }
}
