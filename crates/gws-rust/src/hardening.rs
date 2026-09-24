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

//! Process hardening applied at startup (SEC-17, SEC-18).
//!
//! * umask `0077`: every file and directory gwsr creates (config, token
//!   cache, logs, downloads) is private to the user by default.
//! * `RLIMIT_CORE = 0` and, on Linux, `PR_SET_DUMPABLE = 0`: a crash cannot
//!   write tokens or keys held in memory to a core dump, and other processes
//!   of the same user cannot ptrace-read them.

/// Apply all hardening steps. Steps the OS refuses are returned as warnings
/// (logged by `main` once logging is up); the command still runs because the
/// refusal does not make it unsafe to continue, only less hardened.
#[must_use = "refused hardening steps must be reported"]
pub fn apply() -> Vec<String> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut warnings = Vec::new();
    #[cfg(unix)]
    {
        rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));

        let no_core = rustix::process::Rlimit {
            current: Some(0),
            maximum: Some(0),
        };
        if let Err(e) = rustix::process::setrlimit(rustix::process::Resource::Core, no_core) {
            warnings.push(format!("could not disable core dumps: {e}"));
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Err(e) =
            rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        {
            warnings.push(format!("could not mark the process non-dumpable: {e}"));
        }
    }
    warnings
}
