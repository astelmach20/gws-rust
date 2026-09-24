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

//! Fallible, typed access to parsed clap arguments — the only way commands
//! read their flags.
//!
//! clap's `get_one`/`get_flag` panic when an argument id is not defined on the
//! command or has another type, and `try_get_*(..).ok()` turns the same
//! programming error into a silently absent value. These accessors report it
//! instead, as an internal error (exit code 5) naming the argument.
//!
//! The `*_if_defined` variants are for code shared by commands that do not
//! all define the argument (generated API methods, `+reply` vs `+reply-all`):
//! there an undefined id legitimately reads as absent, and only a type
//! mismatch is an error.

use std::any::Any;

use clap::ArgMatches;
use clap::parser::MatchesError;

use crate::error::GwsError;

fn lookup_error(name: &str, e: &MatchesError) -> GwsError {
    GwsError::other(anyhow::anyhow!(
        "internal error: cannot read argument --{name}: {e}"
    ))
}

/// Treat an undefined argument as absent; report every other lookup error.
fn tolerate_undefined<T>(
    name: &str,
    result: Result<Option<T>, MatchesError>,
) -> Result<Option<T>, GwsError> {
    match result {
        Ok(v) => Ok(v),
        Err(MatchesError::UnknownArgument { .. }) => Ok(None),
        Err(e) => Err(lookup_error(name, &e)),
    }
}

/// A typed single value (`None` when not given).
pub(crate) fn value<'a, T: Any + Clone + Send + Sync + 'static>(
    m: &'a ArgMatches,
    name: &str,
) -> Result<Option<&'a T>, GwsError> {
    m.try_get_one::<T>(name).map_err(|e| lookup_error(name, &e))
}

/// A typed argument declared with a default value, so it always has one.
pub(crate) fn defaulted<T: Any + Clone + Send + Sync + 'static>(
    m: &ArgMatches,
    name: &str,
) -> Result<T, GwsError> {
    value::<T>(m, name)?.cloned().ok_or_else(|| {
        GwsError::other(anyhow::anyhow!(
            "internal error: --{name} has no value (missing default)"
        ))
    })
}

/// [`value`] for an argument that some commands sharing the code do not define.
pub(crate) fn value_if_defined<'a, T: Any + Clone + Send + Sync + 'static>(
    m: &'a ArgMatches,
    name: &str,
) -> Result<Option<&'a T>, GwsError> {
    tolerate_undefined(name, m.try_get_one::<T>(name))
}

/// An optional string argument.
pub(crate) fn optional<'a>(m: &'a ArgMatches, name: &str) -> Result<Option<&'a str>, GwsError> {
    Ok(value::<String>(m, name)?.map(String::as_str))
}

/// [`optional`] for an argument that some commands sharing the code do not define.
pub(crate) fn optional_if_defined<'a>(
    m: &'a ArgMatches,
    name: &str,
) -> Result<Option<&'a str>, GwsError> {
    Ok(value_if_defined::<String>(m, name)?.map(String::as_str))
}

/// A string argument the user must supply.
pub(crate) fn required<'a>(m: &'a ArgMatches, name: &str) -> Result<&'a str, GwsError> {
    optional(m, name)?.ok_or_else(|| GwsError::Validation(format!("--{name} is required")))
}

/// All values of a repeatable string argument (empty when not given).
pub(crate) fn many(m: &ArgMatches, name: &str) -> Result<Vec<String>, GwsError> {
    Ok(m.try_get_many::<String>(name)
        .map_err(|e| lookup_error(name, &e))?
        .map(|v| v.cloned().collect())
        .unwrap_or_default())
}

/// The raw values of a repeatable string argument (`None` when not given).
pub(crate) fn values<'a>(
    m: &'a ArgMatches,
    name: &str,
) -> Result<Option<clap::parser::ValuesRef<'a, String>>, GwsError> {
    m.try_get_many::<String>(name)
        .map_err(|e| lookup_error(name, &e))
}

/// A boolean (`SetTrue`) flag.
pub(crate) fn flag(m: &ArgMatches, name: &str) -> Result<bool, GwsError> {
    Ok(value::<bool>(m, name)?.copied().unwrap_or(false))
}

/// [`flag`] for a flag that some commands sharing the code do not define.
pub(crate) fn flag_if_defined(m: &ArgMatches, name: &str) -> Result<bool, GwsError> {
    Ok(value_if_defined::<bool>(m, name)?.copied().unwrap_or(false))
}

/// Whether the user supplied the argument at all (any type). An argument the
/// command does not define was not supplied.
pub(crate) fn present_if_defined(m: &ArgMatches, name: &str) -> Result<bool, GwsError> {
    match m.try_contains_id(name) {
        Ok(v) => Ok(v),
        Err(MatchesError::UnknownArgument { .. }) => Ok(false),
        Err(e) => Err(lookup_error(name, &e)),
    }
}

/// The global `--dry-run` flag.
pub(crate) fn dry_run(m: &ArgMatches) -> Result<bool, GwsError> {
    flag(m, "dry-run")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, ArgAction, Command};

    fn matches(args: &[&str]) -> ArgMatches {
        Command::new("t")
            .arg(Arg::new("name").long("name"))
            .arg(Arg::new("tag").long("tag").action(ArgAction::Append))
            .arg(Arg::new("yes").long("yes").action(ArgAction::SetTrue))
            .arg(
                Arg::new("count")
                    .long("count")
                    .value_parser(clap::value_parser!(u32)),
            )
            .try_get_matches_from(args)
            .unwrap()
    }

    #[test]
    fn reads_defined_arguments() {
        let m = matches(&["t", "--name", "a", "--tag", "x", "--tag", "y", "--yes"]);
        assert_eq!(optional(&m, "name").unwrap(), Some("a"));
        assert_eq!(required(&m, "name").unwrap(), "a");
        assert_eq!(many(&m, "tag").unwrap(), vec!["x", "y"]);
        assert!(flag(&m, "yes").unwrap());
        assert_eq!(value::<u32>(&m, "count").unwrap(), None);
        let empty = matches(&["t"]);
        assert!(!flag(&empty, "yes").unwrap());
        assert!(many(&empty, "tag").unwrap().is_empty());
        assert!(matches!(
            required(&empty, "name"),
            Err(GwsError::Validation(ref m)) if m == "--name is required"
        ));
    }

    #[test]
    fn undefined_arguments_are_loud_errors() {
        let m = matches(&["t", "--name", "a"]);
        for err in [
            flag(&m, "nope").unwrap_err(),
            optional(&m, "nope").unwrap_err(),
            many(&m, "nope").unwrap_err(),
            required(&m, "nope").unwrap_err(),
            dry_run(&m).unwrap_err(),
        ] {
            assert!(matches!(err, GwsError::Other(_)), "{err:?}");
            assert!(err.to_string().contains("--"), "{err}");
        }
    }

    #[test]
    fn type_mismatches_are_loud_errors_even_if_defined_variant() {
        let m = matches(&["t", "--name", "a"]);
        // `name` is a String, not a bool.
        assert!(flag(&m, "name").is_err());
        assert!(flag_if_defined(&m, "name").is_err());
        assert!(optional_if_defined(&m, "yes").is_err());
    }

    #[test]
    fn if_defined_variants_read_undefined_as_absent() {
        let m = matches(&["t"]);
        assert!(!flag_if_defined(&m, "nope").unwrap());
        assert_eq!(optional_if_defined(&m, "nope").unwrap(), None);
        assert!(!present_if_defined(&m, "nope").unwrap());
        let m = matches(&["t", "--count", "3"]);
        assert!(present_if_defined(&m, "count").unwrap());
        assert_eq!(value_if_defined::<u32>(&m, "count").unwrap(), Some(&3));
    }
}
