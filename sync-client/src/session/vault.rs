use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) struct JournalVault<V: Vault> {
    inner: V,
    marker: std::path::PathBuf,
}
impl<V: Vault> JournalVault<V> {
    pub(super) fn new(inner: V, marker: std::path::PathBuf) -> Self {
        Self { inner, marker }
    }
    fn clear(&self) -> Result<(), SessionError> {
        match std::fs::remove_file(&self.marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(SessionError::CredentialStore),
        }
    }
}
impl<V: Vault> Vault for JournalVault<V> {
    fn load(&self) -> Result<Option<String>, SessionError> {
        let value = self.inner.load()?;
        if self
            .marker
            .try_exists()
            .map_err(|_| SessionError::CredentialStore)?
        {
            if let Some(raw) = value {
                let raw = Zeroizing::new(raw);
                let mut saved: Saved =
                    serde_json::from_str(&raw).map_err(|_| SessionError::CredentialStore)?;
                saved.phase = Phase::Blocked;
                return serde_json::to_string(&saved)
                    .map(Some)
                    .map_err(|_| SessionError::CredentialStore);
            }
            return Ok(None);
        }
        Ok(value)
    }
    fn save(&self, value: &str) -> Result<(), SessionError> {
        let saved: Saved =
            serde_json::from_str(value).map_err(|_| SessionError::CredentialStore)?;
        if saved.phase == Phase::Blocked {
            File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&self.marker)
                .and_then(|file| file.sync_all())
                .map_err(|_| SessionError::CredentialStore)?;
        }
        self.inner.save(value)?;
        if saved.phase == Phase::Ready {
            self.clear()?;
        }
        Ok(())
    }
    fn delete(&self) -> Result<(), SessionError> {
        self.inner.delete()?;
        self.clear()
    }
}

pub(super) trait Vault {
    fn load(&self) -> Result<Option<String>, SessionError>;
    fn save(&self, value: &str) -> Result<(), SessionError>;
    fn delete(&self) -> Result<(), SessionError>;
}

pub(super) struct PlatformVault {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    entry: keyring::Entry,
}

fn key(scope: &Scope) -> String {
    let bytes = serde_json::to_vec(scope).expect("Scope serialization cannot fail.");
    format!("{:x}", Sha256::digest(bytes))
}
fn lock(path: &Path) -> Result<File, SessionError> {
    let mut options = File::options();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|_| SessionError::CredentialStore)?;
    file.try_lock().map_err(|_| SessionError::Busy)?;
    Ok(file)
}
impl PlatformVault {
    pub(super) fn open(scope: &Scope) -> Result<(JournalVault<Self>, File), SessionError> {
        let dirs = directories::ProjectDirs::from("com", "Trackfast", "Trackfast")
            .ok_or(SessionError::CredentialStore)?;
        let directory = dirs.data_local_dir().join("session-locks");
        std::fs::create_dir_all(&directory).map_err(|_| SessionError::CredentialStore)?;
        let key = key(scope);
        let lock = lock(&directory.join(format!("{key}.lock")))?;
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            let entry = keyring::Entry::new("Trackfast API sessions v1", &key)
                .map_err(|_| SessionError::CredentialStore)?;
            Ok((
                JournalVault::new(Self { entry }, directory.join(format!("{key}.blocked"))),
                lock,
            ))
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = lock;
            Err(SessionError::CredentialStore)
        }
    }
}
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl Vault for PlatformVault {
    fn load(&self) -> Result<Option<String>, SessionError> {
        match self.entry.get_password() {
            Ok(value) if value.len() <= 4096 => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            _ => Err(SessionError::CredentialStore),
        }
    }
    fn save(&self, value: &str) -> Result<(), SessionError> {
        self.entry
            .set_password(value)
            .map_err(|_| SessionError::CredentialStore)
    }
    fn delete(&self) -> Result<(), SessionError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            _ => Err(SessionError::CredentialStore),
        }
    }
}
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
impl Vault for PlatformVault {
    fn load(&self) -> Result<Option<String>, SessionError> {
        Err(SessionError::CredentialStore)
    }
    fn save(&self, _: &str) -> Result<(), SessionError> {
        Err(SessionError::CredentialStore)
    }
    fn delete(&self) -> Result<(), SessionError> {
        Err(SessionError::CredentialStore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_lock_excludes_another_instance_and_releases_on_drop() {
        let path = std::env::temp_dir().join(format!("trackfast-session-lock-{}", Uuid::new_v4()));
        let first = lock(&path).unwrap();
        assert!(matches!(lock(&path), Err(SessionError::Busy)));
        drop(first);
        drop(lock(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn credential_names_separate_servers_tenants_and_people() {
        let scope = super::super::tests::scope();
        let original = key(&scope);
        let mut other = scope.clone();
        other.person_id = Uuid::new_v4();
        assert_ne!(original, key(&other));
        other = scope.clone();
        other.tenant_id = Uuid::new_v4();
        assert_ne!(original, key(&other));
        other = scope;
        other.server = "http://127.0.0.1:8789".into();
        assert_ne!(original, key(&other));
    }
    #[test]
    #[ignore = "Requires an unlocked operating system credential store. Uses a generated test account."]
    fn native_vault_roundtrip() {
        let scope = super::super::tests::scope();
        let (vault, lock) = PlatformVault::open(&scope).unwrap();
        assert!(vault.load().unwrap().is_none());
        let mut saved = Saved {
            version: 1,
            scope,
            phase: Phase::Blocked,
            refresh_token: "a".repeat(64),
        };
        vault.save(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(vault.marker.exists());
        let first: Saved = serde_json::from_str(&vault.load().unwrap().unwrap()).unwrap();
        assert_eq!(first.phase, Phase::Blocked);
        saved.phase = Phase::Ready;
        saved.refresh_token = "b".repeat(64);
        vault.save(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(!vault.marker.exists());
        let second: Saved = serde_json::from_str(&vault.load().unwrap().unwrap()).unwrap();
        assert_eq!(second.phase, Phase::Ready);
        assert_eq!(second.refresh_token, "b".repeat(64));
        vault.delete().unwrap();
        vault.delete().unwrap();
        assert!(vault.load().unwrap().is_none());
        drop(lock);
    }
}
