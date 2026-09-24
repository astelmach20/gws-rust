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

//! Process hardening for a program that holds OAuth secrets in memory.

/// Disable core dumps so refresh tokens, keys and access tokens in memory
/// cannot end up in a core file: `RLIMIT_CORE = 0` on Unix, plus
/// `PR_SET_DUMPABLE = 0` on Linux (which also blocks same-user ptrace
/// attachment by non-root processes).
///
/// # Errors
///
/// The OS refused one of the settings.
pub fn disable_core_dumps() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use rustix::process::{Resource, Rlimit, setrlimit};
        setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )?;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn core_limit_is_zero_after_hardening() {
        super::disable_core_dumps().unwrap();
        let limit = rustix::process::getrlimit(rustix::process::Resource::Core);
        assert_eq!(limit.current, Some(0));
        assert_eq!(limit.maximum, Some(0));
    }
}
