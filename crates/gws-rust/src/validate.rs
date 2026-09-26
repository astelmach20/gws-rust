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

//! Input validation: re-exports from the `gws_rust_core` library crate, plus
//! the file-path validators bound to this process's `GWSR_RESTRICT_PATHS`
//! policy (validated by [`crate::env`]).

use std::path::PathBuf;

pub use gws_rust_core::validate::*;

use crate::error::GwsError;

fn current_dir() -> Result<PathBuf, GwsError> {
    std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))
}

fn policy() -> Result<PathPolicy, GwsError> {
    Ok(crate::env::get()?.restrict_paths)
}

/// Validate a file path such as `--upload` or `--output` under the process
/// path policy. See [`resolve_file_path`].
pub fn validate_safe_file_path(path_str: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    resolve_file_path(path_str, flag_name, policy()?, &current_dir()?)
}

/// Validate a single output file path (`-o/--output`) under the process path
/// policy. Like [`validate_safe_file_path`], but an existing directory is
/// refused up front: it can never be replaced by a file, so accepting it
/// would only fail after the whole request and download.
pub fn validate_output_file_path(path_str: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    let path = validate_safe_file_path(path_str, flag_name)?;
    reject_directory(&path, flag_name)?;
    Ok(path)
}

/// Refuse an output file path that names an existing directory.
pub fn reject_directory(path: &std::path::Path, flag_name: &str) -> Result<(), GwsError> {
    if path.is_dir() {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' is a directory; give a file path inside it instead",
            path.display()
        )));
    }
    Ok(())
}

/// Validate an output directory (e.g. `--output-dir`) under the process path
/// policy. See [`resolve_output_dir`].
pub fn validate_safe_output_dir(dir: &str) -> Result<PathBuf, GwsError> {
    resolve_output_dir(dir, policy()?, &current_dir()?)
}

/// Validate an existing directory to read from (e.g. `--dir`) under the
/// process path policy. See [`resolve_dir_path`].
pub fn validate_safe_dir_path(dir: &str) -> Result<PathBuf, GwsError> {
    resolve_dir_path(dir, policy()?, &current_dir()?)
}
