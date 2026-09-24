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

//! The executor's single stdout path.
//!
//! Every line and every byte the executor writes to stdout goes through
//! [`Emitter`]. Tests construct a capturing emitter to assert on output
//! without touching the process's stdout.

use std::io::Write;
use std::sync::Mutex;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::errors::other;
use crate::error::GwsError;
use crate::formatter::OutputFormat;

#[derive(Default)]
pub(crate) struct Emitter {
    /// When set, output is captured here instead of written to stdout.
    captured: Option<Mutex<Vec<u8>>>,
}

impl Emitter {
    pub(crate) fn stdout() -> Self {
        Self { captured: None }
    }

    #[cfg(test)]
    pub(crate) fn capturing() -> Self {
        Self {
            captured: Some(Mutex::new(Vec::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn captured_bytes(&self) -> Vec<u8> {
        self.captured
            .as_ref()
            .and_then(|m| m.lock().ok().map(|v| v.clone()))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn captured_text(&self) -> String {
        String::from_utf8_lossy(&self.captured_bytes()).into_owned()
    }

    fn write_captured(&self, bytes: &[u8]) -> Result<bool, GwsError> {
        match &self.captured {
            Some(buf) => {
                buf.lock()
                    .map_err(|_| other(anyhow::anyhow!("output buffer lock poisoned")))?
                    .extend_from_slice(bytes);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Write one line (a trailing newline is added).
    ///
    /// Merge point with the cli workstream: this becomes `crate::output::emit`.
    pub(crate) fn line(&self, text: &str) -> Result<(), GwsError> {
        let mut data = Vec::with_capacity(text.len() + 1);
        data.extend_from_slice(text.as_bytes());
        data.push(b'\n');
        if self.write_captured(&data)? {
            return Ok(());
        }
        let mut out = std::io::stdout().lock();
        out.write_all(&data)
            .and_then(|()| out.flush())
            .map_err(|e| other(anyhow::Error::new(e).context("failed to write to stdout")))
    }

    /// Write a complete value in `format`.
    ///
    /// Merge point with the cli workstream: `format_value` becomes fallible.
    pub(crate) fn value(&self, value: &Value, format: &OutputFormat) -> Result<(), GwsError> {
        self.line(&crate::formatter::format_value(value, format))
    }

    /// Write one page (or item) of a paginated stream in `format`.
    pub(crate) fn page(
        &self,
        value: &Value,
        format: &OutputFormat,
        first: bool,
    ) -> Result<(), GwsError> {
        self.line(&crate::formatter::format_value_paginated(
            value, format, first,
        ))
    }

    /// Write raw bytes (binary downloads streamed to stdout).
    pub(crate) async fn bytes(&self, data: &[u8]) -> Result<(), GwsError> {
        if self.write_captured(data)? {
            return Ok(());
        }
        let mut out = tokio::io::stdout();
        out.write_all(data)
            .await
            .map_err(|e| other(anyhow::Error::new(e).context("failed to write to stdout")))
    }

    pub(crate) async fn flush(&self) -> Result<(), GwsError> {
        if self.captured.is_some() {
            return Ok(());
        }
        tokio::io::stdout()
            .flush()
            .await
            .map_err(|e| other(anyhow::Error::new(e).context("failed to flush stdout")))
    }
}
