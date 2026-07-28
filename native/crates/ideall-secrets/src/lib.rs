//! Typed, platform-native storage for authentication and encryption secrets.
//!
//! Secret values never pass through the SQLite settings API. Desktop targets
//! use the OS keychain/credential service; iOS uses Keychain; Android uses a
//! Keystore-backed encrypted SharedPreferences store.

use keyring::Entry;
use thiserror::Error;

const SERVICE: &str = "org.ideall.ideall";
const MAX_SECRET_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretKey {
    SyncCode,
    SyncBearerToken,
    AgentCredential,
}

impl SecretKey {
    const fn account(self) -> &'static str {
        match self {
            Self::SyncCode => "sync-code",
            Self::SyncBearerToken => "sync-bearer-token",
            Self::AgentCredential => "agent-credential",
        }
    }
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret value is empty")]
    Empty,
    #[error("secret value exceeds the secure-store budget")]
    TooLarge,
    #[error("platform secure store is unavailable: {0}")]
    Platform(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: SecretKey) -> Result<bool, SecretError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSecretStore;

impl SystemSecretStore {
    fn entry(key: SecretKey) -> Result<Entry, SecretError> {
        Entry::new(SERVICE, key.account()).map_err(platform_error)
    }
}

impl SecretStore for SystemSecretStore {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(platform_error(error)),
        }
    }

    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError> {
        if value.is_empty() {
            return Err(SecretError::Empty);
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err(SecretError::TooLarge);
        }
        Self::entry(key)?
            .set_password(value)
            .map_err(platform_error)
    }

    fn delete(&self, key: SecretKey) -> Result<bool, SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(platform_error(error)),
        }
    }
}

fn platform_error(error: keyring::Error) -> SecretError {
    SecretError::Platform(error.to_string())
}

/// Installs the Android Keystore backend after `android-activity` initialized
/// `ndk-context`. Other platforms are initialized lazily by `keyring`.
#[cfg(target_os = "android")]
pub fn initialize_platform() -> Result<(), SecretError> {
    let store = android_native_keyring_store::Store::new()
        .map_err(|error| SecretError::Platform(error.to_string()))?;
    keyring_core::set_default_store(store);
    Ok(())
}

/// Registers iOS Protected Data (Keychain) as the process-wide credential
/// store. The compatibility `keyring` v1 feature only installs defaults for
/// desktop platforms.
#[cfg(target_os = "ios")]
pub fn initialize_platform() -> Result<(), SecretError> {
    let store = apple_native_keyring_store::protected::Store::new()
        .map_err(|error| SecretError::Platform(error.to_string()))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub const fn initialize_platform() -> Result<(), SecretError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use super::*;

    #[derive(Default)]
    struct MemorySecretStore(Mutex<BTreeMap<&'static str, String>>);

    impl SecretStore for MemorySecretStore {
        fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError> {
            Ok(self.0.lock().unwrap().get(key.account()).cloned())
        }

        fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError> {
            if value.is_empty() {
                return Err(SecretError::Empty);
            }
            if value.len() > MAX_SECRET_BYTES {
                return Err(SecretError::TooLarge);
            }
            self.0
                .lock()
                .unwrap()
                .insert(key.account(), value.to_owned());
            Ok(())
        }

        fn delete(&self, key: SecretKey) -> Result<bool, SecretError> {
            Ok(self.0.lock().unwrap().remove(key.account()).is_some())
        }
    }

    #[test]
    fn typed_store_round_trips_without_a_database() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get(SecretKey::SyncCode).unwrap(), None);
        store
            .set(SecretKey::SyncCode, "0123456789abcdef0123456789abcdef")
            .unwrap();
        assert_eq!(
            store.get(SecretKey::SyncCode).unwrap().as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert!(store.delete(SecretKey::SyncCode).unwrap());
        assert!(!store.delete(SecretKey::SyncCode).unwrap());
    }

    #[test]
    fn rejects_empty_and_oversized_values_before_the_platform_call() {
        let store = MemorySecretStore::default();
        assert!(matches!(
            store.set(SecretKey::AgentCredential, ""),
            Err(SecretError::Empty)
        ));
        let oversized = "x".repeat(MAX_SECRET_BYTES + 1);
        assert!(matches!(
            store.set(SecretKey::SyncBearerToken, &oversized),
            Err(SecretError::TooLarge)
        ));
    }
}
