// Wired into settings commands by the "events and settings" contract work (#8).
#![allow(dead_code)]

//! API key storage (ADR-013, D-09). The key lives in the OS credential store
//! (macOS Keychain, Windows Credential Manager, Secret Service on Linux).
//! Where that store is unavailable, it falls back to process memory for this
//! session only, and reports `KeyStorage::SessionOnly` so the UI can warn.
//!
//! The key never leaves this module except through `SecretString::expose`,
//! which only the provider calls when building a request.

use std::sync::Mutex;

const SERVICE: &str = "dev.jarvis.app";
const ACCOUNT: &str = "anthropic-api-key";
/// Longest key we accept; real keys are ~100 chars.
const MAX_KEY_LEN: usize = 512;
/// Keys shorter than this get no hint, so a hint never reveals a large share.
const MIN_LEN_FOR_HINT: usize = 16;

/// A secret that cannot be printed by accident: no `Display`, and `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Trims surrounding whitespace and rejects empty, oversized or
    /// whitespace-containing input (a pasted key with a stray space or newline).
    pub fn parse(raw: &str) -> Result<Self, KeychainError> {
        let k = raw.trim();
        if k.is_empty() || k.len() > MAX_KEY_LEN || k.chars().any(char::is_whitespace) {
            return Err(KeychainError::InvalidKey);
        }
        Ok(Self(k.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Last 4 characters, e.g. "…a1b2". The only key-derived data shown in the UI (#8).
    pub fn hint(&self) -> Option<String> {
        let chars: Vec<char> = self.0.chars().collect();
        if chars.len() < MIN_LEN_FOR_HINT {
            return None;
        }
        Some(format!(
            "…{}",
            chars[chars.len() - 4..].iter().collect::<String>()
        ))
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(***)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStorage {
    Keychain,
    SessionOnly,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeychainError {
    #[error("the API key is empty, too long or contains spaces")]
    InvalidKey,
    /// Store-specific detail is logged, never shown: it can include paths or account names.
    #[error("the credential store failed")]
    Store,
}

/// One secret slot. Implemented by the OS store and by the in-memory fallback.
pub trait SecretBackend: Send + Sync {
    fn get(&self) -> Result<Option<SecretString>, BackendError>;
    fn set(&self, key: &SecretString) -> Result<(), BackendError>;
    fn delete(&self) -> Result<(), BackendError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum BackendError {
    /// The store exists but cannot be used (no Secret Service, locked, denied):
    /// fall back to session-only.
    Unavailable(String),
    Other(String),
}

pub struct OsKeychain;

impl OsKeychain {
    /// `Ok` if the platform credential store initialised.
    pub fn probe() -> Result<Self, BackendError> {
        keyring::Entry::store_status()
            .as_ref()
            .map(|_| Self)
            .map_err(|e| BackendError::Unavailable(e.to_string()))
    }

    fn entry() -> Result<keyring::Entry, BackendError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(map_keyring)
    }
}

fn map_keyring(e: keyring::Error) -> BackendError {
    use keyring::Error as E;
    match e {
        E::NoStorageAccess(_) | E::PlatformFailure(_) | E::NoDefaultStore => {
            BackendError::Unavailable(e.to_string())
        }
        other => BackendError::Other(other.to_string()),
    }
}

impl SecretBackend for OsKeychain {
    fn get(&self) -> Result<Option<SecretString>, BackendError> {
        match Self::entry()?.get_password() {
            Ok(s) => Ok(SecretString::parse(&s).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring(e)),
        }
    }

    fn set(&self, key: &SecretString) -> Result<(), BackendError> {
        Self::entry()?
            .set_password(key.expose())
            .map_err(map_keyring)
    }

    fn delete(&self) -> Result<(), BackendError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring(e)),
        }
    }
}

/// Session-only fallback: the key is gone when the app quits.
#[derive(Default)]
pub struct SessionMemory(Mutex<Option<SecretString>>);

impl SecretBackend for SessionMemory {
    fn get(&self) -> Result<Option<SecretString>, BackendError> {
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn set(&self, key: &SecretString) -> Result<(), BackendError> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(key.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), BackendError> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

pub struct ApiKeyVault {
    inner: Mutex<VaultInner>,
}

struct VaultInner {
    backend: Box<dyn SecretBackend>,
    storage: KeyStorage,
}

impl ApiKeyVault {
    /// Uses the OS store when it initialises, otherwise session-only.
    pub fn open() -> Self {
        match OsKeychain::probe() {
            Ok(os) => Self::with_backend(Box::new(os), KeyStorage::Keychain),
            Err(e) => {
                tracing::warn!(error = ?e, "OS credential store unavailable; API key will be session-only");
                Self::with_backend(Box::<SessionMemory>::default(), KeyStorage::SessionOnly)
            }
        }
    }

    pub fn with_backend(backend: Box<dyn SecretBackend>, storage: KeyStorage) -> Self {
        Self {
            inner: Mutex::new(VaultInner { backend, storage }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VaultInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn storage(&self) -> KeyStorage {
        self.lock().storage
    }

    pub fn get(&self) -> Result<Option<SecretString>, KeychainError> {
        let g = self.lock();
        g.backend.get().map_err(|e| {
            tracing::warn!(error = ?e, "reading API key failed");
            KeychainError::Store
        })
    }

    /// Stores the key. If the OS store turns out to be unusable at write time
    /// (e.g. Secret Service missing on a minimal Linux desktop), switches to
    /// session-only for the rest of the run and still keeps the key.
    pub fn set(&self, key: &SecretString) -> Result<KeyStorage, KeychainError> {
        let mut g = self.lock();
        match g.backend.set(key) {
            Ok(()) => Ok(g.storage),
            Err(BackendError::Unavailable(e)) if g.storage == KeyStorage::Keychain => {
                tracing::warn!(error = %e, "OS credential store unavailable on write; falling back to session-only");
                let mem = SessionMemory::default();
                mem.set(key).map_err(|_| KeychainError::Store)?;
                g.backend = Box::new(mem);
                g.storage = KeyStorage::SessionOnly;
                Ok(KeyStorage::SessionOnly)
            }
            Err(e) => {
                tracing::warn!(error = ?e, "storing API key failed");
                Err(KeychainError::Store)
            }
        }
    }

    pub fn clear(&self) -> Result<(), KeychainError> {
        self.lock().backend.delete().map_err(|e| {
            tracing::warn!(error = ?e, "deleting API key failed");
            KeychainError::Store
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-ant-test-0123456789abcdefWXYZ";

    #[test]
    fn parse_trims_and_rejects_bad_input() {
        assert_eq!(
            SecretString::parse(&format!("  {KEY}\n")).unwrap().expose(),
            KEY
        );
        for bad in ["", "   ", "sk ant", "a\tb", &"x".repeat(MAX_KEY_LEN + 1)] {
            assert_eq!(
                SecretString::parse(bad),
                Err(KeychainError::InvalidKey),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn debug_never_prints_the_key() {
        let k = SecretString::parse(KEY).unwrap();
        let shown = format!("{k:?} {:?}", Some(k.clone()));
        assert!(!shown.contains("sk-ant"), "{shown}");
        assert!(!shown.contains("WXYZ"), "{shown}");
    }

    #[test]
    fn hint_is_last_four_and_only_for_long_keys() {
        assert_eq!(
            SecretString::parse(KEY).unwrap().hint().as_deref(),
            Some("…WXYZ")
        );
        assert_eq!(SecretString::parse("short-key").unwrap().hint(), None);
    }

    #[test]
    fn session_vault_round_trip() {
        let v = ApiKeyVault::with_backend(Box::<SessionMemory>::default(), KeyStorage::SessionOnly);
        assert_eq!(v.get().unwrap(), None);
        let k = SecretString::parse(KEY).unwrap();
        assert_eq!(v.set(&k).unwrap(), KeyStorage::SessionOnly);
        assert_eq!(v.get().unwrap(), Some(k));
        v.clear().unwrap();
        assert_eq!(v.get().unwrap(), None);
    }

    struct Failing(BackendError);
    impl SecretBackend for Failing {
        fn get(&self) -> Result<Option<SecretString>, BackendError> {
            Err(BackendError::Other("get".into()))
        }
        fn set(&self, _: &SecretString) -> Result<(), BackendError> {
            Err(match &self.0 {
                BackendError::Unavailable(s) => BackendError::Unavailable(s.clone()),
                BackendError::Other(s) => BackendError::Other(s.clone()),
            })
        }
        fn delete(&self) -> Result<(), BackendError> {
            Err(BackendError::Other("delete".into()))
        }
    }

    #[test]
    fn unavailable_keychain_on_write_falls_back_to_session_and_keeps_key() {
        let v = ApiKeyVault::with_backend(
            Box::new(Failing(BackendError::Unavailable(
                "no secret service".into(),
            ))),
            KeyStorage::Keychain,
        );
        let k = SecretString::parse(KEY).unwrap();
        assert_eq!(v.set(&k).unwrap(), KeyStorage::SessionOnly);
        assert_eq!(v.storage(), KeyStorage::SessionOnly);
        assert_eq!(v.get().unwrap(), Some(k));
    }

    #[test]
    fn other_store_errors_are_reported_not_hidden() {
        let v = ApiKeyVault::with_backend(
            Box::new(Failing(BackendError::Other("denied".into()))),
            KeyStorage::Keychain,
        );
        let k = SecretString::parse(KEY).unwrap();
        assert_eq!(v.set(&k), Err(KeychainError::Store));
        assert_eq!(v.storage(), KeyStorage::Keychain);
        assert_eq!(v.get(), Err(KeychainError::Store));
        assert_eq!(v.clear(), Err(KeychainError::Store));
    }

    #[test]
    fn open_never_panics_and_picks_a_storage() {
        // CI has no Secret Service on Linux; either outcome is valid, a panic is not.
        let v = ApiKeyVault::open();
        assert!(matches!(
            v.storage(),
            KeyStorage::Keychain | KeyStorage::SessionOnly
        ));
    }
}
