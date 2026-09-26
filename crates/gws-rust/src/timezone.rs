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

//! Account timezone resolution for Google Workspace CLI.
//!
//! Resolves the authenticated user's timezone with the following priority:
//! 1. Explicit `--timezone` CLI flag (hard error if invalid)
//! 2. Cached value in the gwsr cache directory (24h TTL), one file per
//!    credential identity (see [`crate::auth::identity_cache_key`]); nothing
//!    is cached for a raw access token, whose account is unknown
//! 3. Google Calendar Settings API (`users/me/settings/timezone`)
//! 4. Machine-local timezone (fallback with warning)

use crate::error::GwsError;
use crate::transport::Transport;
use chrono_tz::Tz;
use std::path::{Path, PathBuf};

/// Prefix of the per-identity cache files in the gwsr cache directory.
const CACHE_FILENAME_PREFIX: &str = "account_timezone";

/// The Calendar Settings API endpoint holding the account time zone.
const SETTINGS_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/settings/timezone";

/// Cache TTL in seconds (24 hours).
const CACHE_TTL_SECS: u64 = 86400;

/// The cache file under `root` for the account `identity`, or `None` when
/// the identity is unknown (then nothing is cached). The identity is hashed
/// into the file name, so each account has its own file.
fn cache_file(root: &Path, identity: Option<&str>) -> Option<PathBuf> {
    identity.map(|id| {
        let hash = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, id.as_bytes());
        root.join(format!("{CACHE_FILENAME_PREFIX}-{}", hash.simple()))
    })
}

/// The cache file for the credential this invocation uses.
fn cache_path() -> Result<Option<PathBuf>, GwsError> {
    let identity = crate::auth::identity_cache_key().map_err(crate::auth::to_gws_error)?;
    Ok(cache_file(
        &crate::discovery::gwsr_cache_root()?,
        identity.as_deref(),
    ))
}

/// Remove every cached timezone. Called on auth login/logout to invalidate
/// stale values when the account changes.
pub fn invalidate_cache() -> Result<(), GwsError> {
    invalidate_cache_in(&crate::discovery::gwsr_cache_root()?)
}

fn invalidate_cache_in(root: &Path) -> Result<(), GwsError> {
    let failed = |path: &Path, e: std::io::Error| {
        GwsError::other(anyhow::Error::new(e).context(format!(
            "failed to remove the timezone cache {}",
            path.display()
        )))
    };
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(failed(root, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| failed(root, e))?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(CACHE_FILENAME_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(failed(&path, e));
        }
    }
    Ok(())
}

/// Read the cached timezone if it exists and is fresh (< 24h old).
///
/// The cache is a derived artifact: a missing or stale file is a miss, and an
/// unreadable or corrupt one is reported with a warning and treated as a miss
/// (the value is then re-fetched and the cache rewritten).
fn read_cache(path: &std::path::Path) -> Option<Tz> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot stat timezone cache; ignoring it");
            return None;
        }
    };
    // A cache whose age cannot be determined (no mtime support, or an mtime
    // in the future) is treated as stale and refetched, which is always safe.
    let fresh = metadata
        .modified()
        .ok()
        .and_then(|m| std::time::SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age.as_secs() <= CACHE_TTL_SECS);
    if !fresh {
        return None;
    }
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot read timezone cache; ignoring it");
            return None;
        }
    };
    let tz_name = contents.trim();
    match tz_name.parse::<Tz>() {
        Ok(tz) => Some(tz),
        Err(_) => {
            tracing::warn!(path = %path.display(), value = tz_name, "corrupt timezone cache; ignoring it");
            None
        }
    }
}

/// Write a timezone name to the cache file.
/// A failed write only costs a refetch next time, so it is reported as a
/// warning rather than failing the command that already has its answer.
fn write_cache(path: &std::path::Path, tz_name: &str) {
    let result = path
        .parent()
        .map_or(Ok(()), crate::fs_util::ensure_private_dir)
        .and_then(|()| crate::fs_util::atomic_write(path, tz_name.as_bytes()));
    if let Err(e) = result {
        tracing::warn!(path = %path.display(), error = %e, "failed to write timezone cache");
    }
}

/// Fetch the account timezone from the Google Calendar Settings API at `url`,
/// caching it in `cache` (if any).
async fn fetch_account_timezone(
    transport: &Transport,
    url: &str,
    cache: Option<&Path>,
) -> Result<Tz, GwsError> {
    let json = transport
        .get_json(url, &[], "Failed to fetch account timezone")
        .await?;

    let tz_name = json
        .get("value")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GwsError::other(anyhow::anyhow!(
                "Timezone setting missing or empty 'value' field"
            ))
        })?;

    let tz: Tz = tz_name.parse().map_err(|_| {
        GwsError::other(anyhow::anyhow!(
            "Google returned unrecognized timezone: {tz_name}"
        ))
    })?;

    if let Some(path) = cache {
        write_cache(path, tz_name);
    }
    tracing::info!(
        timezone = tz_name,
        source = "calendar_api",
        "resolved account timezone"
    );

    Ok(tz)
}

/// Parse an explicit timezone string, returning an error if invalid.
pub fn parse_timezone(tz_str: &str) -> Result<Tz, GwsError> {
    tz_str.parse::<Tz>().map_err(|_| {
        GwsError::Validation(format!(
            "Invalid timezone '{tz_str}'. Use an IANA timezone name (e.g. America/Denver, Europe/London, UTC)."
        ))
    })
}

/// Resolve the user's timezone with this priority:
/// 1. `tz_override` (from `--timezone` flag) — hard error if invalid
/// 2. Cached value in config dir — use if < 24h old
/// 3. Google Calendar Settings API — fetch and cache
/// 4. Machine-local timezone (log warning)
pub(crate) async fn resolve_account_timezone(
    transport: &Transport,
    tz_override: Option<&str>,
) -> Result<Tz, GwsError> {
    // 1. Explicit override — fail if invalid
    if let Some(tz_str) = tz_override {
        let tz = parse_timezone(tz_str)?;
        tracing::info!(
            timezone = tz_str,
            source = "cli_flag",
            "using explicit timezone"
        );
        return Ok(tz);
    }
    resolve_with(transport, SETTINGS_URL, cache_path()?.as_deref()).await
}

/// Steps 2-4 of [`resolve_account_timezone`], with the settings URL and the
/// identity's cache file (`None`: do not cache) passed in.
async fn resolve_with(
    transport: &Transport,
    url: &str,
    cache: Option<&Path>,
) -> Result<Tz, GwsError> {
    // 2. Check this identity's cache
    if let Some(tz) = cache.and_then(read_cache) {
        tracing::debug!(timezone = %tz, source = "cache", "using cached timezone");
        return Ok(tz);
    }

    // 3. Fetch from Calendar Settings API
    let api_error = match fetch_account_timezone(transport, url, cache).await {
        Ok(tz) => return Ok(tz),
        Err(e) => e,
    };

    // 4. Fall back to the machine-local timezone, announced on stderr so the
    //    user knows "today" is computed in a possibly different zone.
    let tz = local_timezone().map_err(|local_err| {
        GwsError::Validation(format!(
            "could not determine the account timezone ({api_error}) or the local timezone \
             ({local_err}); pass --timezone <IANA name>"
        ))
    })?;
    tracing::warn!(
        error = %api_error,
        timezone = %tz,
        "could not read the account timezone; using the local timezone"
    );
    Ok(tz)
}

/// Return the start of today (midnight) in the given timezone as a
/// timezone-aware `DateTime`. Errors if midnight cannot be resolved
/// (e.g. a DST transition that skips midnight — extremely rare).
pub fn start_of_today(tz: Tz) -> Result<chrono::DateTime<Tz>, crate::error::GwsError> {
    use chrono::{NaiveTime, TimeZone, Utc};

    let now_in_tz = Utc::now().with_timezone(&tz);
    let today_start = now_in_tz.date_naive().and_time(NaiveTime::MIN);
    tz.from_local_datetime(&today_start)
        .earliest()
        .ok_or_else(|| {
            crate::error::GwsError::other(anyhow::anyhow!(
                "Could not determine start of day in timezone '{}'",
                tz
            ))
        })
}

/// Machine-local IANA timezone, read from the OS by `iana-time-zone`.
fn local_timezone() -> Result<Tz, String> {
    let name = iana_time_zone::get_timezone().map_err(|e| e.to_string())?;
    name.parse::<Tz>()
        .map_err(|_| format!("unrecognized local timezone '{name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_iana_timezone() {
        let tz = parse_timezone("America/Denver").unwrap();
        assert_eq!(tz, chrono_tz::America::Denver);
    }

    #[test]
    fn parse_utc_timezone() {
        let tz = parse_timezone("UTC").unwrap();
        assert_eq!(tz, chrono_tz::UTC);
    }

    #[test]
    fn parse_invalid_timezone_fails() {
        let result = parse_timezone("Not/A/Zone");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid timezone"));
        assert!(err.contains("Not/A/Zone"));
    }

    #[test]
    fn parse_empty_string_fails() {
        let result = parse_timezone("");
        assert!(result.is_err());
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = cache_file(dir.path(), Some("profile:/c:default")).unwrap();
        std::fs::write(&cache_file, "America/New_York\n").unwrap();
        assert_eq!(read_cache(&cache_file), Some(chrono_tz::America::New_York));
    }

    #[test]
    fn missing_or_corrupt_cache_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = cache_file(dir.path(), Some("profile:/c:default")).unwrap();
        assert_eq!(read_cache(&cache_file), None);
        std::fs::write(&cache_file, "Not/A/Zone").unwrap();
        assert_eq!(read_cache(&cache_file), None);
    }

    async fn settings_server(tz: &str) -> wiremock::MockServer {
        use wiremock::matchers::{method, path};
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(method("GET"))
            .and(path("/calendar/v3/users/me/settings/timezone"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "value": tz })),
            )
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    async fn resolve_from(server: &wiremock::MockServer, cache: Option<&Path>) -> Tz {
        let url = format!("{}/calendar/v3/users/me/settings/timezone", server.uri());
        resolve_with(&Transport::for_test(&server.uri()), &url, cache)
            .await
            .unwrap()
    }

    /// Two identities (here two profiles) never share a cached time zone:
    /// switching profile, credentials file or `--impersonate` subject must
    /// not reuse the previous account's zone.
    #[tokio::test]
    async fn cached_timezone_is_never_shared_between_identities() {
        let dir = tempfile::tempdir().unwrap();
        let tokyo = settings_server("Asia/Tokyo").await;
        let new_york = settings_server("America/New_York").await;
        let work = cache_file(dir.path(), Some("profile:/c:work"));
        let personal = cache_file(dir.path(), Some("profile:/c:personal"));

        assert_eq!(
            resolve_from(&tokyo, work.as_deref()).await,
            chrono_tz::Asia::Tokyo
        );
        assert_eq!(
            resolve_from(&new_york, personal.as_deref()).await,
            chrono_tz::America::New_York,
            "the second account must not get the first account's cached zone"
        );
        // Each identity's value is cached (each server expects one call).
        assert_eq!(
            resolve_from(&tokyo, work.as_deref()).await,
            chrono_tz::Asia::Tokyo
        );

        invalidate_cache_in(dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// A raw access token has no known account: its zone is never cached.
    #[tokio::test]
    async fn unknown_identity_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cache_file(dir.path(), None), None);
        let server = settings_server("Asia/Tokyo").await;
        assert_eq!(resolve_from(&server, None).await, chrono_tz::Asia::Tokyo);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn local_timezone_is_parseable_when_detected() {
        // Detection can legitimately fail in minimal containers; when it
        // succeeds the name must be a valid IANA zone.
        if let Ok(tz) = local_timezone() {
            assert!(!tz.name().is_empty());
        }
    }

    #[test]
    fn start_of_today_is_midnight() {
        use chrono::Timelike;
        let start = start_of_today(chrono_tz::America::Denver).unwrap();
        assert_eq!((start.hour(), start.minute(), start.second()), (0, 0, 0));
    }
}
