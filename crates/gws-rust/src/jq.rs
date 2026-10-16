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

//! `--jq <expr>` support, backed by the `jaq` interpreter.
//!
//! The expression is compiled once at startup (so a syntax error fails before
//! any API call) and then applied to every JSON value the CLI prints.

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Ctx, Vars, data};
use jaq_json::{Num, Val};
use serde_json::Value;

use crate::error::GwsError;

type Data = data::JustLut<Val>;

/// jq builtins `--jq` does not offer (see [`JqFilter::compile`]).
const DISABLED_FUNS: &[&str] = &["env"];

/// A compiled jq filter.
pub struct JqFilter {
    source: String,
    filter: jaq_core::Filter<Data>,
}

impl std::fmt::Debug for JqFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JqFilter")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl JqFilter {
    /// Parse and compile a jq expression.
    pub fn compile(expr: &str) -> Result<Self, GwsError> {
        let defs = jaq_core::defs()
            .chain(jaq_std::defs())
            .chain(jaq_json::defs());
        // jq's `env` returns the whole process environment, secrets such as
        // `GWSR_TOKEN` and `GWSR_CLIENT_SECRET` included, which gwsr never
        // prints. A `--jq` expression (often written by an LLM) must not be
        // able to put them on stdout, so `env` is left undefined.
        let funs = jaq_core::funs()
            .chain(jaq_std::funs())
            .chain(jaq_json::funs())
            .filter(|(name, _, _)| !DISABLED_FUNS.contains(name));

        let arena = Arena::default();
        let program = File {
            code: expr,
            path: (),
        };
        let modules = Loader::new(defs).load(&arena, program).map_err(|errs| {
            let details: Vec<String> = errs.iter().map(|(_, e)| format!("{e:?}")).collect();
            GwsError::Validation(format!(
                "invalid --jq expression '{expr}': syntax error ({})",
                details.join("; ")
            ))
        })?;
        let filter = jaq_core::Compiler::default()
            .with_funs(funs)
            .compile(modules)
            .map_err(|errs| {
                let details: Vec<String> = errs
                    .iter()
                    .flat_map(|(_, es)| es.iter().map(|e| format!("{e:?}")))
                    .collect();
                GwsError::Validation(format!(
                    "invalid --jq expression '{expr}': undefined name ({})",
                    details.join("; ")
                ))
            })?;
        Ok(Self {
            source: expr.to_string(),
            filter,
        })
    }

    /// Run the filter on `input`, returning every output value in order.
    pub fn run(&self, input: &Value) -> Result<Vec<Value>, GwsError> {
        let text = serde_json::to_string(input).map_err(|e| {
            GwsError::from(anyhow::Error::new(e).context("failed to serialize --jq input"))
        })?;
        let val = jaq_json::read::parse_single(text.as_bytes())
            .map_err(|e| GwsError::Validation(format!("failed to convert value for --jq: {e}")))?;

        let ctx = Ctx::<Data>::new(&self.filter.lut, Vars::new([]));
        let mut outputs = Vec::new();
        for result in self.filter.id.run((ctx, val)) {
            let out = match result {
                Ok(v) => v,
                Err(exn) => {
                    let message = match exn.get_err() {
                        Ok(err) => err.to_string(),
                        Err(exn) => match exn.get_halt() {
                            Ok(code) => format!("halt({code})"),
                            Err(_) => "unexpected jq control flow".to_string(),
                        },
                    };
                    return Err(GwsError::Validation(format!(
                        "--jq '{}' failed: {message}",
                        self.source
                    )));
                }
            };
            let rendered = jq_compatible(out).to_string();
            let value: Value = serde_json::from_str(&rendered).map_err(|e| {
                GwsError::Validation(format!(
                    "--jq '{}' produced a value that is not valid JSON ({rendered}): {e}",
                    self.source
                ))
            })?;
            outputs.push(value);
        }
        Ok(outputs)
    }
}

/// jaq prints the non-finite floats `nan`, `infinite` and `-infinite` as
/// `NaN`, `Infinity` and `-Infinity`, which are not JSON. jq prints `null`
/// for NaN and the largest finite double, with its sign, for an infinity;
/// so does `--jq`. A value without such numbers is returned unchanged.
fn jq_compatible(val: Val) -> Val {
    fn non_finite(val: &Val) -> bool {
        match val {
            Val::Num(Num::Float(f)) => !f.is_finite(),
            Val::Arr(a) => a.iter().any(non_finite),
            Val::Obj(o) => o.iter().any(|(k, v)| non_finite(k) || non_finite(v)),
            _ => false,
        }
    }
    fn replace(val: Val) -> Val {
        match val {
            Val::Num(Num::Float(f)) if f.is_nan() => Val::Null,
            Val::Num(Num::Float(f)) if f.is_infinite() => {
                Val::Num(Num::Float(f64::MAX.copysign(f)))
            }
            Val::Arr(a) => Val::Arr(jaq_json::Rc::new(a.iter().cloned().map(replace).collect())),
            Val::Obj(o) => Val::Obj(jaq_json::Rc::new(
                o.iter()
                    .map(|(k, v)| (replace(k.clone()), replace(v.clone())))
                    .collect(),
            )),
            other => other,
        }
    }
    if non_finite(&val) { replace(val) } else { val }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(expr: &str, input: Value) -> Result<Vec<Value>, GwsError> {
        JqFilter::compile(expr)?.run(&input)
    }

    #[test]
    fn identity_returns_input() {
        let out = run(".", json!({"a": 1})).unwrap();
        assert_eq!(out, vec![json!({"a": 1})]);
    }

    #[test]
    fn iterates_arrays() {
        let out = run(".files[].id", json!({"files": [{"id": "a"}, {"id": "b"}]})).unwrap();
        assert_eq!(out, vec![json!("a"), json!("b")]);
    }

    #[test]
    fn std_functions_are_available() {
        let out = run(
            "[.files[] | select(.n > 1)] | length",
            json!({"files": [{"n": 1}, {"n": 2}, {"n": 3}]}),
        )
        .unwrap();
        assert_eq!(out, vec![json!(2)]);
        let out = run("keys", json!({"b": 1, "a": 2})).unwrap();
        assert_eq!(out, vec![json!(["a", "b"])]);
    }

    #[test]
    fn syntax_error_is_validation_error() {
        let err = JqFilter::compile(".files[").unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(err.to_string().contains("invalid --jq expression"));
    }

    #[test]
    fn undefined_function_is_rejected_at_compile_time() {
        assert!(JqFilter::compile("nosuchfn").is_err());
    }

    #[test]
    fn runtime_error_is_surfaced() {
        let err = run(".a.b", json!({"a": 5})).unwrap_err();
        assert!(err.to_string().contains("failed"), "{err}");
    }

    #[test]
    fn empty_produces_no_outputs() {
        assert!(run("empty", json!(1)).unwrap().is_empty());
    }

    /// jq prints `nan` as `null` and an infinity as the largest finite
    /// double with its sign, at any depth; jaq's `NaN`/`Infinity` are not
    /// JSON.
    #[test]
    fn non_finite_numbers_print_like_jq() {
        assert_eq!(run("nan", json!(null)).unwrap(), vec![json!(null)]);
        assert_eq!(run("infinite", json!(null)).unwrap(), vec![json!(f64::MAX)]);
        assert_eq!(
            run("-infinite", json!(null)).unwrap(),
            vec![json!(-f64::MAX)]
        );
        assert_eq!(
            run("{a: [nan, infinite, 1.5], b: .}", json!("x")).unwrap(),
            vec![json!({"a": [null, f64::MAX, 1.5], "b": "x"})]
        );
    }

    /// `env` would print the whole process environment, secrets such as
    /// `GWSR_TOKEN` and `GWSR_CLIENT_SECRET` included, to stdout.
    #[test]
    fn env_is_not_available() {
        for expr in ["env", "env.GWSR_TOKEN", "[env | keys[]]"] {
            let err = JqFilter::compile(expr).unwrap_err();
            assert!(matches!(err, GwsError::Validation(_)), "{expr}: {err:?}");
            assert!(err.to_string().contains("env"), "{expr}: {err}");
        }
        assert!(JqFilter::compile("$ENV").is_err());
        // Other std functions are still there.
        assert_eq!(
            run("now | type", json!(null)).unwrap(),
            vec![json!("number")]
        );
    }
}
