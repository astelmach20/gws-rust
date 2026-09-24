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

//! Encryption-at-rest for credentials and token caches.
//!
//! # Key storage
//!
//! The 256-bit data key lives in exactly one place, chosen by
//! `GWSR_KEYRING_BACKEND`:
//!
//! * `keyring` (default): the OS credential store (macOS Keychain, Windows
//!   Credential Manager, Secret Service on Linux/BSD). Any keyring failure is
//!   an error with guidance; there is no silent fallback to a file.
//! * `file`: `<config>/encryption.key`, which must be a regular file readable
//!   only by its owner (0600). This protects the data only as well as file
//!   permissions do, and is intended for containers and CI.
//!
//! Keys are never copied between backends. A key is generated only while
//! holding a cross-process lock, and never while encrypted data already
//! exists (that data would become unreadable).
//!
//! # Ciphertext format (version 1)
//!
//! ```text
//! "GWSR" | 0x01 | nonce (12 bytes) | AES-256-GCM ciphertext+tag
//! AAD = "GWSR" | 0x01 | purpose label ("credentials" or "token-cache")
//! ```
//!
//! Binding the purpose into the AAD means a token cache can never be decrypted
//! as a credentials file (or vice versa).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use aes_gcm::aead::{Aead, Generate, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 4] = b"GWSR";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Keyring service name for the data key.
pub const KEYRING_SERVICE: &str = "gwsr";
/// Keyring account name for the data key.
pub const KEYRING_ACCOUNT: &str = "encryption-key";
/// File name of the data key when `GWSR_KEYRING_BACKEND=file`.
pub const KEY_FILE_NAME: &str = "encryption.key";
const KEY_LOCK_FILE_NAME: &str = "encryption.key.lock";
const KEY_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// A 256-bit data key, zeroized on drop.
pub type DataKey = Zeroizing<[u8; 32]>;

/// What a ciphertext protects; bound into the AEAD associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Credentials,
    TokenCache,
}

impl Purpose {
    fn label(self) -> &'static [u8] {
        match self {
            Purpose::Credentials => b"credentials",
            Purpose::TokenCache => b"token-cache",
        }
    }

    fn aad(self) -> Vec<u8> {
        let mut aad = Vec::with_capacity(HEADER_LEN + 16);
        aad.extend_from_slice(MAGIC);
        aad.push(FORMAT_VERSION);
        aad.extend_from_slice(self.label());
        aad
    }
}

/// Errors from the key store and the ciphertext format.
#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    /// The data key could not be obtained. Encrypted files are untouched.
    #[error("{0}")]
    KeyUnavailable(String),
    /// Authentication failed: wrong key or tampered/corrupt data.
    #[error(
        "cannot decrypt {what}: authentication failed. The file was encrypted with a different \
         key (another machine, a different GWSR_KEYRING_BACKEND, or a replaced key) or it is \
         corrupt. The file has been left untouched"
    )]
    Decrypt { what: String },
    /// Not a gwsr v1 ciphertext.
    #[error("{what} is not in a supported encrypted format: {detail}")]
    UnsupportedFormat { what: String, detail: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("encryption failed: {0}")]
    Encrypt(String),
}

fn io_err(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> KeystoreError {
    let context = context.into();
    move |source| KeystoreError::Io { context, source }
}

/// Encrypt `plaintext` for `purpose` with `key` into the v1 format.
///
/// # Errors
///
/// Fails only if the system RNG or cipher fails.
pub fn encrypt(
    key: &DataKey,
    purpose: Purpose,
    plaintext: &[u8],
) -> Result<Vec<u8>, KeystoreError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| KeystoreError::Encrypt(e.to_string()))?;
    let nonce = Nonce::try_generate().map_err(|e| KeystoreError::Encrypt(e.to_string()))?;
    let aad = purpose.aad();
    let ct = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|e| KeystoreError::Encrypt(e.to_string()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Check that `data` looks like a v1 ciphertext without needing the key.
///
/// # Errors
///
/// Returns [`KeystoreError::UnsupportedFormat`] describing the mismatch.
pub fn check_format(data: &[u8], what: &str) -> Result<(), KeystoreError> {
    let unsupported = |detail: &str| KeystoreError::UnsupportedFormat {
        what: what.to_string(),
        detail: detail.to_string(),
    };
    if data.len() < HEADER_LEN || &data[..MAGIC.len()] != MAGIC {
        return Err(unsupported("missing the GWSR header"));
    }
    if data[MAGIC.len()] != FORMAT_VERSION {
        return Err(unsupported(&format!(
            "format version {} (this gwsr supports version {FORMAT_VERSION})",
            data[MAGIC.len()]
        )));
    }
    if data.len() < HEADER_LEN + NONCE_LEN + TAG_LEN {
        return Err(unsupported("truncated"));
    }
    Ok(())
}

/// Decrypt a v1 ciphertext for `purpose`.
///
/// # Errors
///
/// [`KeystoreError::UnsupportedFormat`] for foreign data,
/// [`KeystoreError::Decrypt`] for a wrong key, wrong purpose or tampering.
pub fn decrypt(
    key: &DataKey,
    purpose: Purpose,
    data: &[u8],
    what: &str,
) -> Result<Zeroizing<Vec<u8>>, KeystoreError> {
    check_format(data, what)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| KeystoreError::Encrypt(e.to_string()))?;
    let nonce_bytes = &data[HEADER_LEN..HEADER_LEN + NONCE_LEN];
    let nonce = Nonce::try_from(nonce_bytes).map_err(|_| KeystoreError::UnsupportedFormat {
        what: what.to_string(),
        detail: "bad nonce length".to_string(),
    })?;
    let aad = purpose.aad();
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &data[HEADER_LEN + NONCE_LEN..],
                aad: &aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| KeystoreError::Decrypt {
            what: what.to_string(),
        })
}

/// Which backend holds the data key (`GWSR_KEYRING_BACKEND`, validated by
/// [`crate::env`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// The OS credential store (the default).
    #[default]
    Keyring,
    /// A 0600 key file in the config directory.
    File,
}

impl BackendKind {
    /// Parse `keyring` or `file`; anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "keyring" => Some(BackendKind::Keyring),
            "file" => Some(BackendKind::File),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Keyring => "keyring",
            BackendKind::File => "file",
        }
    }
}

/// Storage for the data key.
pub trait KeyBackend: Send + Sync {
    /// Short description for messages, e.g. `OS keyring` or a file path.
    fn describe(&self) -> String;
    /// Load the key. `Ok(None)` means "definitely not present".
    fn load(&self) -> Result<Option<DataKey>, KeystoreError>;
    /// Persist a newly generated key.
    fn store(&self, key: &DataKey) -> Result<(), KeystoreError>;
}

fn keyring_guidance(base: &Path) -> String {
    format!(
        "If this machine has no usable OS keyring (an SSH session with a locked login keychain, \
         headless Linux without a Secret Service, a container or CI), set \
         GWSR_KEYRING_BACKEND=file to keep the key in '{}' instead (protected only by file \
         permissions).",
        base.join(KEY_FILE_NAME).display()
    )
}

/// The OS credential store via the `keyring` crate.
pub struct OsKeyringBackend {
    base: PathBuf,
}

impl OsKeyringBackend {
    pub fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, KeystoreError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| {
            let detail = match (&e, keyring::Entry::store_status()) {
                (keyring::Error::NoDefaultStore, Err(init)) => init.to_string(),
                _ => e.to_string(),
            };
            KeystoreError::KeyUnavailable(format!(
                "the OS keyring is not available: {}. {}",
                crate::output::sanitize_for_terminal(&detail),
                keyring_guidance(&self.base)
            ))
        })
    }

    fn failure(&self, action: &str, e: &keyring::Error) -> KeystoreError {
        KeystoreError::KeyUnavailable(format!(
            "failed to {action} the gwsr encryption key in the OS keyring \
             (service '{KEYRING_SERVICE}', account '{KEYRING_ACCOUNT}'): {}. Nothing was deleted. {}",
            crate::output::sanitize_for_terminal(&e.to_string()),
            keyring_guidance(&self.base)
        ))
    }
}

impl KeyBackend for OsKeyringBackend {
    fn describe(&self) -> String {
        format!("the OS keyring (service '{KEYRING_SERVICE}', account '{KEYRING_ACCOUNT}')")
    }

    fn load(&self) -> Result<Option<DataKey>, KeystoreError> {
        match self.entry()?.get_password() {
            Ok(b64) => {
                let b64 = Zeroizing::new(b64);
                decode_key(b64.trim()).map(Some).map_err(|detail| {
                    KeystoreError::KeyUnavailable(format!(
                        "the gwsr encryption key in the OS keyring is malformed ({detail}). \
                         Nothing was changed; inspect or remove the '{KEYRING_SERVICE}' / \
                         '{KEYRING_ACCOUNT}' keyring item manually"
                    ))
                })
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(self.failure("read", &e)),
        }
    }

    fn store(&self, key: &DataKey) -> Result<(), KeystoreError> {
        let b64 = Zeroizing::new(B64.encode(key.as_ref()));
        self.entry()?
            .set_password(&b64)
            .map_err(|e| self.failure("store", &e))
    }
}

/// A 0600 key file (`GWSR_KEYRING_BACKEND=file`).
pub struct FileKeyBackend {
    path: PathBuf,
}

impl FileKeyBackend {
    pub fn new(base: &Path) -> Self {
        Self {
            path: base.join(KEY_FILE_NAME),
        }
    }
}

impl KeyBackend for FileKeyBackend {
    fn describe(&self) -> String {
        format!("the key file '{}'", self.path.display())
    }

    fn load(&self) -> Result<Option<DataKey>, KeystoreError> {
        let meta = match std::fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(io_err(format!("cannot stat '{}'", self.path.display()))(e));
            }
        };
        if !meta.file_type().is_file() {
            return Err(KeystoreError::KeyUnavailable(format!(
                "refusing to use '{}': the encryption key must be a regular file, not a symlink \
                 or directory",
                self.path.display()
            )));
        }
        crate::fs_util::check_private_file(&self.path)
            .map_err(|e| KeystoreError::KeyUnavailable(format!("{e:#}")))?;
        let contents = Zeroizing::new(
            std::fs::read_to_string(&self.path)
                .map_err(io_err(format!("cannot read '{}'", self.path.display())))?,
        );
        decode_key(contents.trim()).map(Some).map_err(|detail| {
            KeystoreError::KeyUnavailable(format!(
                "the encryption key file '{}' is malformed ({detail}). It was left untouched",
                self.path.display()
            ))
        })
    }

    fn store(&self, key: &DataKey) -> Result<(), KeystoreError> {
        use std::io::Write;
        let parent = self.path.parent().unwrap_or(Path::new("."));
        crate::fs_util::ensure_private_dir(parent)
            .map_err(io_err(format!("cannot create '{}'", parent.display())))?;
        let ctx = format!("cannot create '{}'", self.path.display());
        // Write the complete key to a private temp file first, then publish it
        // with a hard link. Linking never overwrites an existing key and makes
        // the file appear fully written, so a concurrent unlocked `load` can
        // never observe a partially written (e.g. empty) key file.
        let mut tmp = tempfile::Builder::new()
            .prefix(".encryption.key.")
            .tempfile_in(parent)
            .map_err(io_err(ctx.clone()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(io_err(ctx.clone()))?;
        }
        let b64 = Zeroizing::new(B64.encode(key.as_ref()));
        tmp.write_all(b64.as_bytes()).map_err(io_err(ctx.clone()))?;
        tmp.as_file().sync_all().map_err(io_err(ctx.clone()))?;
        std::fs::hard_link(tmp.path(), &self.path).map_err(io_err(ctx.clone()))?;
        // Dropping `tmp` removes the temp name; the published link remains.
        tmp.close().map_err(io_err(ctx.clone()))?;
        crate::fs_util::sync_dir(parent).map_err(io_err(ctx))
    }
}

fn decode_key(b64: &str) -> Result<DataKey, String> {
    let mut decoded = B64
        .decode(b64)
        .map_err(|e| format!("not valid base64: {e}"))?;
    let result = if decoded.len() == 32 {
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&decoded);
        Ok(key)
    } else {
        Err(format!("expected 32 bytes, found {}", decoded.len()))
    };
    decoded.zeroize();
    result
}

/// The data key for one config directory, loaded at most once per instance.
pub struct Keystore {
    backend: Box<dyn KeyBackend>,
    kind: BackendKind,
    base: PathBuf,
    key: OnceLock<DataKey>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("backend", &self.backend.describe())
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Keystore {
    /// Keystore for `base` using the backend `kind`.
    pub fn for_kind(base: &Path, kind: BackendKind) -> Self {
        let backend: Box<dyn KeyBackend> = match kind {
            BackendKind::Keyring => Box::new(OsKeyringBackend::new(base)),
            BackendKind::File => Box::new(FileKeyBackend::new(base)),
        };
        Self::with_backend(base, kind, backend)
    }

    /// Keystore with an explicit backend (used by tests).
    pub fn with_backend(base: &Path, kind: BackendKind, backend: Box<dyn KeyBackend>) -> Self {
        Self {
            backend,
            kind,
            base: base.to_path_buf(),
            key: OnceLock::new(),
        }
    }

    pub fn kind(&self) -> BackendKind {
        self.kind
    }

    pub fn describe(&self) -> String {
        self.backend.describe()
    }

    /// The existing key; never creates one. Used for decryption.
    ///
    /// # Errors
    ///
    /// [`KeystoreError::KeyUnavailable`] if the backend fails or has no key.
    pub fn existing_key(&self) -> Result<&DataKey, KeystoreError> {
        if let Some(key) = self.key.get() {
            return Ok(key);
        }
        match self.backend.load()? {
            Some(key) => Ok(self.key.get_or_init(|| key)),
            None => Err(KeystoreError::KeyUnavailable(format!(
                "the gwsr encryption key was not found in {}, so existing encrypted files cannot \
                 be read. Nothing was deleted. If you changed GWSR_KEYRING_BACKEND, change it \
                 back. If the key is permanently lost, run `gwsr auth logout` and log in again",
                self.backend.describe()
            ))),
        }
    }

    /// The key for encryption, creating it on first use.
    ///
    /// Creation happens under a cross-process lock and is refused while any
    /// encrypted file exists in the config directory.
    ///
    /// # Errors
    ///
    /// Backend failures, lock timeouts, or existing encrypted data without a key.
    pub fn key_for_encryption(&self) -> Result<&DataKey, KeystoreError> {
        if let Some(key) = self.key.get() {
            return Ok(key);
        }
        if let Some(key) = self.backend.load()? {
            return Ok(self.key.get_or_init(|| key));
        }

        crate::fs_util::ensure_private_dir(&self.base)
            .map_err(io_err(format!("cannot create '{}'", self.base.display())))?;
        let lock_path = self.base.join(KEY_LOCK_FILE_NAME);
        let _lock = crate::fs_util::FileLock::acquire(&lock_path, KEY_LOCK_TIMEOUT)
            .map_err(io_err("cannot lock the encryption key for creation"))?;

        // Another process may have created the key while we waited.
        if let Some(key) = self.backend.load()? {
            return Ok(self.key.get_or_init(|| key));
        }

        let existing = encrypted_files(&self.base)
            .map_err(io_err(format!("cannot scan '{}'", self.base.display())))?;
        if !existing.is_empty() {
            let list = existing
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(KeystoreError::KeyUnavailable(format!(
                "the gwsr encryption key was not found in {}, but encrypted data exists:\n{list}\n\
                 gwsr will not generate a new key because that data would become permanently \
                 unreadable. If you changed GWSR_KEYRING_BACKEND, change it back. If the key is \
                 permanently lost, remove those files (`gwsr auth logout`) and log in again",
                self.backend.describe()
            )));
        }

        let mut key = Zeroizing::new([0u8; 32]);
        rand::fill(key.as_mut_slice());
        self.backend.store(&key)?;

        // Read back to prove the key really persisted (ephemeral or mock
        // stores would otherwise silently lose it after this process).
        match self.backend.load()? {
            Some(stored) if stored.as_ref() == key.as_ref() => {
                tracing::debug!(backend = %self.backend.describe(), "created encryption key");
                Ok(self.key.get_or_init(|| key))
            }
            Some(_) => Err(KeystoreError::KeyUnavailable(format!(
                "a different key appeared in {} while creating one; retry the command",
                self.backend.describe()
            ))),
            None => Err(KeystoreError::KeyUnavailable(format!(
                "{} accepted the new encryption key but did not return it when read back, so it \
                 would not persist. {}",
                self.backend.describe(),
                keyring_guidance(&self.base)
            ))),
        }
    }

    /// Encrypt with the (possibly new) key.
    ///
    /// # Errors
    ///
    /// See [`Keystore::key_for_encryption`] and [`encrypt`].
    pub fn encrypt(&self, purpose: Purpose, plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
        encrypt(self.key_for_encryption()?, purpose, plaintext)
    }

    /// Decrypt with the existing key.
    ///
    /// The format is checked before the key is fetched, so foreign files are
    /// reported as such without touching the keyring.
    ///
    /// # Errors
    ///
    /// See [`Keystore::existing_key`] and [`decrypt`].
    pub fn decrypt(
        &self,
        purpose: Purpose,
        data: &[u8],
        what: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeystoreError> {
        check_format(data, what)?;
        decrypt(self.existing_key()?, purpose, data, what)
    }
}

/// Encrypted files (`*.enc`) under `<base>/profiles/*/`.
///
/// # Errors
///
/// Fails if a directory exists but cannot be read.
pub fn encrypted_files(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    let profiles = base.join("profiles");
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(&profiles) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&dir)? {
            let path = file?.path();
            if path.extension().is_some_and(|ext| ext == "enc") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory backend shared between clones.
    #[derive(Clone, Default)]
    pub struct MemoryBackend(pub Arc<Mutex<Option<[u8; 32]>>>);

    impl KeyBackend for MemoryBackend {
        fn describe(&self) -> String {
            "memory".into()
        }
        fn load(&self) -> Result<Option<DataKey>, KeystoreError> {
            Ok(self.0.lock().unwrap().map(Zeroizing::new))
        }
        fn store(&self, key: &DataKey) -> Result<(), KeystoreError> {
            *self.0.lock().unwrap() = Some(**key);
            Ok(())
        }
    }

    /// Backend whose every operation fails like a locked keychain.
    pub struct FailingBackend;

    impl KeyBackend for FailingBackend {
        fn describe(&self) -> String {
            "failing keyring".into()
        }
        fn load(&self) -> Result<Option<DataKey>, KeystoreError> {
            Err(KeystoreError::KeyUnavailable(
                "keyring locked (simulated)".into(),
            ))
        }
        fn store(&self, _key: &DataKey) -> Result<(), KeystoreError> {
            Err(KeystoreError::KeyUnavailable(
                "keyring locked (simulated)".into(),
            ))
        }
    }

    /// Backend that accepts writes but forgets them (like a mock store).
    pub struct ForgetfulBackend;

    impl KeyBackend for ForgetfulBackend {
        fn describe(&self) -> String {
            "forgetful keyring".into()
        }
        fn load(&self) -> Result<Option<DataKey>, KeystoreError> {
            Ok(None)
        }
        fn store(&self, _key: &DataKey) -> Result<(), KeystoreError> {
            Ok(())
        }
    }

    pub fn memory_keystore(base: &Path) -> (Keystore, MemoryBackend) {
        let backend = MemoryBackend::default();
        (
            Keystore::with_backend(base, BackendKind::Keyring, Box::new(backend.clone())),
            backend,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn key(byte: u8) -> DataKey {
        Zeroizing::new([byte; 32])
    }

    #[test]
    fn round_trip_and_format_header() {
        let k = key(7);
        let ct = encrypt(&k, Purpose::Credentials, b"secret").unwrap();
        assert_eq!(&ct[..4], b"GWSR");
        assert_eq!(ct[4], 1);
        let pt = decrypt(&k, Purpose::Credentials, &ct, "x").unwrap();
        assert_eq!(pt.as_slice(), b"secret");
    }

    #[test]
    fn purpose_is_bound_into_aad() {
        let k = key(7);
        let ct = encrypt(&k, Purpose::TokenCache, b"tokens").unwrap();
        let err = decrypt(&k, Purpose::Credentials, &ct, "creds").unwrap_err();
        assert!(matches!(err, KeystoreError::Decrypt { .. }), "{err}");
    }

    #[test]
    fn wrong_key_and_tampering_fail_authentication() {
        let ct = encrypt(&key(1), Purpose::Credentials, b"secret").unwrap();
        assert!(matches!(
            decrypt(&key(2), Purpose::Credentials, &ct, "x"),
            Err(KeystoreError::Decrypt { .. })
        ));
        let mut tampered = ct.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(
            decrypt(&key(1), Purpose::Credentials, &tampered, "x"),
            Err(KeystoreError::Decrypt { .. })
        ));
    }

    #[test]
    fn foreign_and_future_formats_are_rejected() {
        let k = key(1);
        for data in [&b"garbage-without-header"[..], b"GWS", b"GWSR\x01short"] {
            assert!(matches!(
                decrypt(&k, Purpose::Credentials, data, "x"),
                Err(KeystoreError::UnsupportedFormat { .. })
            ));
        }
        let mut v2 = encrypt(&k, Purpose::Credentials, b"s").unwrap();
        v2[4] = 2;
        let err = decrypt(&k, Purpose::Credentials, &v2, "x").unwrap_err();
        assert!(err.to_string().contains("version 2"), "{err}");
    }

    #[test]
    fn backend_parsing() {
        assert_eq!(BackendKind::parse("keyring"), Some(BackendKind::Keyring));
        assert_eq!(BackendKind::parse("file"), Some(BackendKind::File));
        assert_eq!(BackendKind::parse("File "), None);
        assert_eq!(BackendKind::parse("plaintext"), None);
        assert_eq!(BackendKind::default(), BackendKind::Keyring);
    }

    #[test]
    fn creates_key_once_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let (ks, backend) = memory_keystore(dir.path());
        let first = **ks.key_for_encryption().unwrap();
        assert_eq!(backend.0.lock().unwrap().unwrap(), first);
        let ks2 = Keystore::with_backend(dir.path(), BackendKind::Keyring, Box::new(backend));
        assert_eq!(**ks2.existing_key().unwrap(), first);
    }

    #[test]
    fn refuses_to_create_key_while_encrypted_data_exists() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profiles").join("default");
        std::fs::create_dir_all(&profile).unwrap();
        let creds = profile.join("credentials.enc");
        std::fs::write(&creds, b"GWSR\x01...").unwrap();

        let (ks, backend) = memory_keystore(dir.path());
        let err = ks.key_for_encryption().unwrap_err().to_string();
        assert!(err.contains("will not generate a new key"), "{err}");
        assert!(err.contains("credentials.enc"), "{err}");
        assert!(backend.0.lock().unwrap().is_none(), "no key may be created");
        assert!(creds.exists(), "data must survive");
    }

    #[test]
    fn failing_keyring_is_an_error_not_a_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let ks = Keystore::with_backend(dir.path(), BackendKind::Keyring, Box::new(FailingBackend));
        assert!(matches!(
            ks.key_for_encryption(),
            Err(KeystoreError::KeyUnavailable(_))
        ));
        assert!(!dir.path().join(KEY_FILE_NAME).exists(), "no file fallback");
    }

    #[test]
    fn forgetful_store_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let ks =
            Keystore::with_backend(dir.path(), BackendKind::Keyring, Box::new(ForgetfulBackend));
        let err = ks.key_for_encryption().unwrap_err().to_string();
        assert!(err.contains("did not return it"), "{err}");
    }

    #[test]
    fn decrypt_checks_format_before_touching_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let ks = Keystore::with_backend(dir.path(), BackendKind::Keyring, Box::new(FailingBackend));
        assert!(matches!(
            ks.decrypt(Purpose::Credentials, b"plain json", "x"),
            Err(KeystoreError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn file_backend_round_trip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FileKeyBackend::new(dir.path());
        assert!(backend.load().unwrap().is_none());
        let k = key(9);
        backend.store(&k).unwrap();
        assert_eq!(*backend.load().unwrap().unwrap(), [9u8; 32]);
        // create_new: never overwrite an existing key file.
        assert!(backend.store(&key(1)).is_err());
        assert_eq!(*backend.load().unwrap().unwrap(), [9u8; 32]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.path().join(KEY_FILE_NAME);
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let err = backend.load().unwrap_err().to_string();
            assert!(err.contains("chmod 600"), "{err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_backend_refuses_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::write(&real, B64.encode([1u8; 32])).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join(KEY_FILE_NAME)).unwrap();
        let err = FileKeyBackend::new(dir.path())
            .load()
            .unwrap_err()
            .to_string();
        assert!(err.contains("regular file"), "{err}");
    }

    #[test]
    fn malformed_key_file_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE_NAME);
        crate::fs_util::atomic_write(&path, b"not-base64!!").unwrap();
        let ks = Keystore::with_backend(
            dir.path(),
            BackendKind::File,
            Box::new(FileKeyBackend::new(dir.path())),
        );
        assert!(ks.key_for_encryption().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not-base64!!");
    }

    #[test]
    fn concurrent_creation_yields_one_key() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FileKeyBackend::new(dir.path());
        drop(backend);
        let base = dir.path().to_path_buf();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let base = base.clone();
                std::thread::spawn(move || {
                    let ks = Keystore::with_backend(
                        &base,
                        BackendKind::File,
                        Box::new(FileKeyBackend::new(&base)),
                    );
                    **ks.key_for_encryption().unwrap()
                })
            })
            .collect();
        let keys: Vec<[u8; 32]> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(keys.windows(2).all(|w| w[0] == w[1]));
    }
}
