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
    if !is_trusted(&parsed, override_base) {
        return Err(GwsError::Validation(format!(
            "Refusing to use untrusted API base URL '{url}': it must be https://*.googleapis.com/. \
             Set {API_BASE_URL_ENV} to allow a private endpoint explicitly."
        )));
    }
    Ok(parsed)
}

/// The single trust rule: same origin as the operator override, or `https`
/// on the default port to `googleapis.com` / `*.googleapis.com`.
fn is_trusted(parsed: &Url, override_base: Option<&Url>) -> bool {
    if let Some(base) = override_base
        && parsed.origin() == base.origin()
    {
        return true;
    }
    let google_host = match parsed.host() {
        Some(url::Host::Domain(host)) => is_google_api_host(host),
        _ => false,
    };
    parsed.scheme() == "https" && parsed.port().is_none() && google_host
}

/// Decides which request URLs may receive credentials.
///
/// Holds the validated `GWSR_API_BASE_URL` override (if any) and applies the
/// same trust rule as [`validate_api_base`] to full request URLs, which may
/// carry a query string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointPolicy {
    override_base: Option<Url>,
}

impl EndpointPolicy {
    /// Trust Google API hosts only.
    pub fn google_only() -> Self {
        Self::default()
    }

    /// Trust Google API hosts plus the origin of `base`
    /// (see [`parse_api_base_override`] for what `base` may be).
    pub fn with_override(base: &str) -> Result<Self, GwsError> {
        let override_base = parse_api_base_override(Some(base))?.ok_or_else(|| {
            GwsError::Validation(format!("{API_BASE_URL_ENV} override must not be empty"))
        })?;
        Ok(Self {
            override_base: Some(override_base),
        })
    }

    /// Read `GWSR_API_BASE_URL`; unset or empty means [`Self::google_only`].
    pub fn from_env() -> Result<Self, GwsError> {
        Ok(Self {
            override_base: api_base_override()?,
        })
    }

    /// The operator-supplied base URL (always ends in `/`).
    pub fn override_base(&self) -> Option<&Url> {
        self.override_base.as_ref()
    }

    /// Validate that `url` may receive a bearer token. Returns the parsed URL.
    pub fn check(&self, url: &str) -> Result<Url, GwsError> {
        let parsed = Url::parse(url).map_err(|e| {
            GwsError::Validation(format!(
                "Refusing to send credentials to invalid URL {url:?}: {e}"
            ))
        })?;
        let clean_userinfo = parsed.username().is_empty() && parsed.password().is_none();
        if clean_userinfo
            && parsed.fragment().is_none()
            && is_trusted(&parsed, self.override_base())
        {
            return Ok(parsed);
        }
        let mut shown = parsed.clone();
        shown.set_query(None);
        shown.set_fragment(None);
        // Never echo embedded credentials.
        let _ = shown.set_username("");
        let _ = shown.set_password(None);
        Err(GwsError::Validation(format!(
            "Refusing to send credentials to {shown}: only https://*.googleapis.com is trusted \
             (set {API_BASE_URL_ENV} to route requests to another endpoint)"
        )))
    }
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

    #[test]
    fn endpoint_policy_default_trusts_googleapis_https_only() {
        let p = EndpointPolicy::google_only();
        assert!(
            p.check("https://www.googleapis.com/drive/v3/files?q=a")
                .is_ok()
        );
        assert!(p.check("https://gmail.googleapis.com/gmail/v1/x").is_ok());
        for bad in [
            "http://www.googleapis.com/drive/v3/files",
            "https://evil.com/drive",
            "https://googleapis.com.evil.com/",
            "https://evilgoogleapis.com/",
            "https://www.googleapis.com:8443/x",
            "https://user:pw@www.googleapis.com/x",
            "https://www.googleapis.com/x#frag",
        ] {
            let err = p.check(bad).unwrap_err().to_string();
            assert!(!err.contains("pw"), "{err}");
        }
    }

    #[test]
    fn endpoint_policy_override() {
        let p = EndpointPolicy::with_override("https://proxy.corp.example/api").unwrap();
        assert_eq!(
            p.override_base().unwrap().as_str(),
            "https://proxy.corp.example/api/"
        );
        assert!(
            p.check("https://proxy.corp.example/api/drive/v3/files")
                .is_ok()
        );
        assert!(p.check("https://other.example/").is_err());
        assert!(p.check("https://www.googleapis.com/x").is_ok());

        assert!(EndpointPolicy::with_override("http://127.0.0.1:8080").is_ok());
        assert!(EndpointPolicy::with_override("http://localhost:8080/").is_ok());
        for bad in [
            "http://proxy.corp.example/",
            "https://user:pw@proxy.example/",
            "https://proxy.example/?a=b",
            "ftp://proxy.example/",
            "not a url",
            "",
        ] {
            assert!(EndpointPolicy::with_override(bad).is_err(), "{bad}");
        }
    }
}
