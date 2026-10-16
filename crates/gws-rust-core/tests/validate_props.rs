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

//! Property-based (fuzz-style) tests for the untrusted-input parsers in
//! `gws_rust_core::validate`, `services`, and Discovery deserialization.

use gws_rust_core::discovery::RestDescription;
use gws_rust_core::services::resolve_service_spec;
use gws_rust_core::validate::{
    ModelArmorTemplate, PathPolicy, encode_path_preserving_slashes, encode_path_segment,
    parse_api_base_override, reject_dangerous_chars, resolve_file_path, validate_api_base_with,
    validate_api_identifier, validate_resource_name,
};
use percent_encoding::percent_decode_str;
use proptest::prelude::*;

/// Code points `reject_dangerous_chars` must refuse besides C0, DEL and C1,
/// written out here rather than taken from `is_dangerous_unicode`, so the
/// property below checks the definition instead of restating it.
const DANGEROUS: &[u32] = &[
    0x061C, 0x180E, 0x200B, 0x200C, 0x200D, 0x200E, 0x200F, 0x2028, 0x2029, 0x202A, 0x202B, 0x202C,
    0x202D, 0x202E, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064, 0x2066, 0x2067, 0x2068, 0x2069, 0xFEFF,
];

fn must_refuse(c: char) -> bool {
    let cp = u32::from(c);
    cp < 0x20 || (0x7F..=0x9F).contains(&cp) || DANGEROUS.contains(&cp)
}

/// Strings mixing arbitrary characters with the refused ones and their
/// neighbours, which `any::<String>()` almost never produces.
fn mixed_string() -> impl Strategy<Value = String> {
    let near: Vec<char> = DANGEROUS
        .iter()
        .flat_map(|&cp| [cp - 1, cp, cp + 1])
        .chain([0x1F, 0x20, 0x7E, 0x7F, 0x9F, 0xA0])
        .filter_map(char::from_u32)
        .collect();
    prop::collection::vec(
        prop_oneof![any::<char>(), prop::sample::select(near), Just('a')],
        0..6,
    )
    .prop_map(|cs| cs.into_iter().collect())
}

/// URLs assembled from parts that cover every accept and reject branch of the
/// API base checks, so the properties below see accepted URLs too.
fn base_url() -> impl Strategy<Value = (String, BaseParts)> {
    (
        prop::sample::select(vec!["https", "HTTPS", "http", "ftp"]),
        prop::sample::select(vec!["", "user@", "user:pw@"]),
        prop::sample::select(vec![
            "googleapis.com",
            "www.googleapis.com",
            "x.googleapis.com.",
            "googleapis.com.evil.com",
            "evilgoogleapis.com",
            "localhost",
            "LOCALHOST",
            "localhost.evil.com",
            "127.0.0.1",
            "127.8.9.10",
            "[::1]",
            "10.0.0.1",
            "example.com",
        ]),
        prop::sample::select(vec!["", ":443", ":8080"]),
        prop::sample::select(vec!["", "/", "/v1", "/a/b/"]),
        prop::sample::select(vec!["", "?q=1", "#f"]),
    )
        .prop_map(|(scheme, user, host, port, path, tail)| {
            let url = format!("{scheme}://{user}{host}{port}{path}{tail}");
            let parts = BaseParts {
                scheme: scheme.to_ascii_lowercase(),
                clean: user.is_empty() && tail.is_empty(),
                google: host.trim_end_matches('.') == "googleapis.com"
                    || host.trim_end_matches('.').ends_with(".googleapis.com"),
                loopback: matches!(
                    host,
                    "localhost" | "LOCALHOST" | "127.0.0.1" | "127.8.9.10" | "[::1]"
                ),
                default_port: port.is_empty()
                    || (port == ":443" && scheme.eq_ignore_ascii_case("https")),
            };
            (url, parts)
        })
}

#[derive(Debug, Clone)]
struct BaseParts {
    scheme: String,
    clean: bool,
    google: bool,
    loopback: bool,
    default_port: bool,
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 2048,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn encode_path_segment_is_safe_and_roundtrips(s in any::<String>()) {
        let enc = encode_path_segment(&s);
        prop_assert!(enc.bytes().all(|b| b.is_ascii_alphanumeric() || b"%-._~@".contains(&b)));
        prop_assert!(enc != "." && enc != "..");
        prop_assert_eq!(percent_decode_str(&enc).decode_utf8().unwrap(), s.as_str());
    }

    #[test]
    fn encode_preserving_slashes_keeps_segments(s in any::<String>()) {
        let enc = encode_path_preserving_slashes(&s);
        prop_assert!(enc.bytes().all(|b| b.is_ascii_alphanumeric() || b"%-._~@/".contains(&b)));
        prop_assert!(enc.split('/').all(|seg| seg != "." && seg != ".."));
        prop_assert_eq!(enc.matches('/').count(), s.matches('/').count());
        prop_assert_eq!(percent_decode_str(&enc).decode_utf8().unwrap(), s.as_str());
    }

    #[test]
    fn accepted_resource_names_are_url_safe(s in any::<String>()) {
        if let Ok(name) = validate_resource_name(&s) {
            prop_assert_eq!(name, s.as_str());
            if name != "@default" {
                prop_assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || "._~/-".contains(c)));
                prop_assert!(name.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".."));
            }
        }
    }

    #[test]
    fn resource_names_from_safe_alphabet_are_accepted(
        segs in prop::collection::vec("[A-Za-z0-9_~-][A-Za-z0-9._~-]{0,15}", 1..6)
    ) {
        let name = segs.join("/");
        prop_assume!(name.split('/').all(|seg| seg != "." && seg != ".."));
        prop_assert!(validate_resource_name(&name).is_ok(), "{}", name);
    }

    #[test]
    fn accepted_api_identifiers_are_safe_filenames(s in any::<String>()) {
        if let Ok(id) = validate_api_identifier(&s) {
            prop_assert!(!id.is_empty() && id.len() <= 100);
            prop_assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)));
            prop_assert!(!id.contains("..") && !id.starts_with(['.', '-', '_']));
        }
    }

    #[test]
    fn reject_dangerous_chars_matches_definition(s in mixed_string()) {
        let expected_ok = !s.chars().any(must_refuse);
        prop_assert_eq!(reject_dangerous_chars(&s, "--x").is_ok(), expected_ok, "{:?}", s);
    }

    #[test]
    fn service_specs_never_yield_unsafe_identifiers(s in any::<String>(), v in proptest::option::of(any::<String>())) {
        if let Ok(r) = resolve_service_spec(&s, v.as_deref()) {
            prop_assert!(validate_api_identifier(&r.api_name).is_ok());
            prop_assert!(validate_api_identifier(&r.version).is_ok());
        }
    }

    #[test]
    fn trusted_api_bases_are_https_googleapis((s, p) in base_url()) {
        let expected_ok = p.scheme == "https" && p.clean && p.google && p.default_port;
        let result = validate_api_base_with(&s, None);
        prop_assert_eq!(result.is_ok(), expected_ok, "{} {:?}", s, result);
        if let Ok(url) = result {
            prop_assert_eq!(url.scheme(), "https");
            let host = url.host_str().unwrap().trim_end_matches('.').to_string();
            prop_assert!(host == "googleapis.com" || host.ends_with(".googleapis.com"), "{}", host);
            prop_assert!(url.username().is_empty() && url.password().is_none());
            prop_assert!(url.port().is_none() && url.query().is_none() && url.fragment().is_none());
        }
    }

    #[test]
    fn arbitrary_api_bases_are_refused_or_https_googleapis(s in any::<String>()) {
        if let Ok(url) = validate_api_base_with(&s, None) {
            prop_assert_eq!(url.scheme(), "https");
            let host = url.host_str().unwrap().trim_end_matches('.').to_string();
            prop_assert!(host == "googleapis.com" || host.ends_with(".googleapis.com"), "{}", host);
        }
    }

    #[test]
    fn crafted_hosts_around_googleapis_are_rejected(prefix in "[a-z0-9.@:-]{0,20}", suffix in "[a-z0-9.@:/-]{1,20}") {
        let url = format!("https://{prefix}googleapis.com.{suffix}");
        if let Ok(u) = validate_api_base_with(&url, None) {
            let host = u.host_str().unwrap().trim_end_matches('.').to_string();
            prop_assert!(host.ends_with(".googleapis.com") || host == "googleapis.com", "{}", host);
        }
    }

    #[test]
    fn api_base_override_is_https_or_loopback((s, p) in base_url()) {
        let expected_ok = p.clean && (p.scheme == "https" || (p.scheme == "http" && p.loopback));
        let result = parse_api_base_override(Some(&s));
        prop_assert_eq!(result.is_ok(), expected_ok, "{} {:?}", s, result);
        if let Ok(url) = result {
            let url = url.unwrap();
            prop_assert!(url.path().ends_with('/'));
            prop_assert!(url.username().is_empty() && url.query().is_none());
        }
    }

    #[test]
    fn model_armor_hosts_stay_on_rep_googleapis(s in any::<String>()) {
        if let Ok(t) = ModelArmorTemplate::parse(&s) {
            let url = t.api_base().unwrap();
            prop_assert!(url.host_str().unwrap().ends_with(".rep.googleapis.com"));
            prop_assert_eq!(t.name(), s);
        }
    }

    #[test]
    fn model_armor_rejects_non_grammar_locations(loc in "[^a-z0-9-]{1,5}") {
        let name = format!("projects/my-proj-1/locations/{loc}/templates/t");
        prop_assert!(ModelArmorTemplate::parse(&name).is_err());
    }

    #[test]
    fn discovery_deserialization_never_panics(s in any::<String>()) {
        let _parsed: Result<RestDescription, _> = serde_json::from_str(&s);
    }

    #[test]
    fn strict_path_policy_never_escapes_cwd(
        segs in prop::collection::vec(prop_oneof![Just("..".to_string()), Just(".".to_string()), "[a-z]{1,6}"], 1..8)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let rel = segs.join("/");
        if let Ok(p) = resolve_file_path(&rel, "--output", PathPolicy::Cwd, &cwd) {
            prop_assert!(p.starts_with(&cwd), "{} -> {}", rel, p.display());
        }
        let p = resolve_file_path(&rel, "--output", PathPolicy::Unrestricted, &cwd).unwrap();
        prop_assert!(p.is_absolute());
        prop_assert!(!p.components().any(|c| matches!(c, std::path::Component::ParentDir)));
    }
}
