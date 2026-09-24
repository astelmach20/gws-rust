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

//! API endpoint trust checks.
//!
//! Discovery Documents carry the `rootUrl` that requests (and bearer tokens)
//! are sent to. Because documents are cached on disk, a tampered cache could
//! otherwise redirect credentials. [`validate_api_base`] only accepts:
//!
//! - `https://` URLs on the default port whose host is `googleapis.com` or a
//!   subdomain of it (this covers regional/mTLS hosts such as
//!   `modelarmor.us-central1.rep.googleapis.com`), or
//! - URLs with the same origin as an explicit user override in
//!   `GWSR_API_BASE_URL` (for private endpoints, VPC-SC, or recording proxies).
//!
//! User info, query strings, and fragments are always rejected.

use reqwest::Url;

use crate::error::GwsError;

/// Environment variable holding an explicit API base URL override.
pub const API_BASE_URL_ENV: &str = "GWSR_API_BASE_URL";

/// Whether `host` is `googleapis.com` or one of its subdomains.
pub fn is_google_api_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "googleapis.com" || host.ends_with(".googleapis.com")
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn parse_url(raw: &str, what: &str) -> Result<Url, GwsError> {
    let url = Url::parse(raw).map_err(|e| {
        GwsError::Validation(format!(
            "{what} '{}' is not a valid URL: {e}",
            raw.escape_debug()
        ))
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(GwsError::Validation(format!(
            "{what} must not contain credentials"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(GwsError::Validation(format!(
            "{what} '{raw}' must not contain a query string or fragment"
        )));
    }
    if url.host().is_none() {
        return Err(GwsError::Validation(format!("{what} '{raw}' has no host")));
    }
    Ok(url)
}

/// Parse a `GWSR_API_BASE_URL` value.
///
/// Unset or empty means no override. The URL must be `https://`, or
/// `http://` on a loopback host (for local recording proxies), and must not
/// contain credentials, a query, or a fragment. The returned URL's path always
/// ends with `/` so it can be used as a `rootUrl`.
pub fn parse_api_base_override(value: Option<&str>) -> Result<Option<Url>, GwsError> {
    let raw = match value.map(str::trim) {
        None | Some("") => return Ok(None),
        Some(v) => v,
    };
    let mut url = parse_url(raw, API_BASE_URL_ENV)?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        other => {
            return Err(GwsError::Validation(format!(
                "{API_BASE_URL_ENV} must use https (http is only allowed for localhost), got '{other}'"
            )));
        }
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(Some(url))
}

/// Read and validate `GWSR_API_BASE_URL`. See [`parse_api_base_override`].
pub fn api_base_override() -> Result<Option<Url>, GwsError> {
    match std::env::var(API_BASE_URL_ENV) {
        Ok(v) => parse_api_base_override(Some(&v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(GwsError::Validation(format!(
            "{API_BASE_URL_ENV} is not valid UTF-8"
        ))),
    }
}

/// Validate that `url` is a trusted API base (e.g. a Discovery `rootUrl`)
/// that credentials may be sent to, honoring `GWSR_API_BASE_URL`.
pub fn validate_api_base(url: &str) -> Result<Url, GwsError> {
    validate_api_base_with(url, api_base_override()?.as_ref())
}

/// Like [`validate_api_base`], with the override passed explicitly.
pub fn validate_api_base_with(url: &str, override_base: Option<&Url>) -> Result<Url, GwsError> {
    let parsed = parse_url(url, "API base URL")?;
    if let Some(base) = override_base
        && parsed.origin() == base.origin()
    {
        return Ok(parsed);
    }
    let host = parsed.host_str().unwrap_or_default();
    let trusted_host =
        matches!(parsed.host(), Some(url::Host::Domain(_))) && is_google_api_host(host);
    if parsed.scheme() != "https" || parsed.port().is_some() || !trusted_host {
        return Err(GwsError::Validation(format!(
            "Refusing to use untrusted API base URL '{url}': it must be https://*.googleapis.com/. \
             Set {API_BASE_URL_ENV} to allow a private endpoint explicitly."
        )));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_hosts_are_trusted() {
        for ok in [
            "https://www.googleapis.com/",
            "https://admin.googleapis.com/",
            "https://drive.mtls.googleapis.com/",
            "https://modelarmor.us-central1.rep.googleapis.com/",
            "https://googleapis.com/",
            "https://WWW.GoogleAPIs.com/drive/v3/",
        ] {
            assert!(validate_api_base_with(ok, None).is_ok(), "{ok}");
        }
    }

    #[test]
    fn untrusted_urls_are_rejected() {
        for bad in [
            "http://www.googleapis.com/",
            "https://evil.com/",
            "https://googleapis.com.evil.com/",
            "https://evilgoogleapis.com/",
            "https://www.googleapis.com:8443/",
            "https://user:pw@www.googleapis.com/",
            "https://www.googleapis.com/?x=1",
            "https://www.googleapis.com/#frag",
            "https://127.0.0.1/",
            "file:///etc/passwd",
            "not a url",
            "",
        ] {
            let err = validate_api_base_with(bad, None);
            assert!(matches!(err, Err(GwsError::Validation(_))), "{bad}");
        }
    }

    #[test]
    fn override_allows_matching_origin_only() {
        let base = parse_api_base_override(Some("https://proxy.corp.example/google"))
            .unwrap()
            .unwrap();
        assert_eq!(base.as_str(), "https://proxy.corp.example/google/");
        assert!(validate_api_base_with("https://proxy.corp.example/other/", Some(&base)).is_ok());
        assert!(validate_api_base_with("https://evil.example/", Some(&base)).is_err());
        // Google hosts remain trusted with an override set.
        assert!(validate_api_base_with("https://www.googleapis.com/", Some(&base)).is_ok());
    }

    #[test]
    fn override_parsing() {
        assert_eq!(parse_api_base_override(None).unwrap(), None);
        assert_eq!(parse_api_base_override(Some("  ")).unwrap(), None);
        assert!(
            parse_api_base_override(Some("http://localhost:8080"))
                .unwrap()
                .is_some()
        );
        assert!(
            parse_api_base_override(Some("http://127.0.0.1:9000/"))
                .unwrap()
                .is_some()
        );
        assert!(
            parse_api_base_override(Some("http://[::1]:9000/"))
                .unwrap()
                .is_some()
        );
        for bad in [
            "http://proxy.example/",
            "ftp://proxy.example/",
            "https://u:p@proxy.example/",
            "https://proxy.example/?a=b",
            "proxy.example",
        ] {
            assert!(parse_api_base_override(Some(bad)).is_err(), "{bad}");
        }
    }
}
