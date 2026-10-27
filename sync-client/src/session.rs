#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;

mod http;
mod vault;
use crate::{
    Queue, Scope,
    transport::{DevelopmentClient, Page, Project, RemoteEntry, SyncError},
};
use http::LocalAuthApi;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    time::{Duration, Instant},
};
use uuid::Uuid;
use vault::{JournalVault, PlatformVault, Vault};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug)]
pub enum SessionError {
    InvalidConfiguration,
    CredentialStore,
    Busy,
    SignInRequired,
    AlreadySignedIn,
    RevocationPending,
    Unavailable,
    InvalidResponse,
    IdentityMismatch,
    Sync(SyncError),
}
impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => {
                "Use a canonical local API address and valid account IDs."
            }
            Self::CredentialStore => {
                "Credential storage failed. Retry cleanup before another sign-in."
            }
            Self::Busy => "Another client owns this account session. Close that client first.",
            Self::SignInRequired => "Sign in again. Complete pending sign-out cleanup first.",
            Self::AlreadySignedIn => "Sign out before replacing this session.",
            Self::RevocationPending => {
                "Local access is blocked. Retry sign-out when the API is available."
            }
            Self::Unavailable => "The API request failed. The session state may require cleanup.",
            Self::InvalidResponse => "The API returned an invalid session response.",
            Self::IdentityMismatch => "The session belongs to another account or server.",
            Self::Sync(_) => "The sync operation failed. Local pending work remains available.",
        })
    }
}
impl std::error::Error for SessionError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub challenge_id: Uuid,
    pub nonce: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenPair {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
}
impl TokenPair {
    fn validate(&self) -> Result<(), SessionError> {
        if !valid_token(&self.access_token)
            || !valid_token(&self.refresh_token)
            || self.token_type != "Bearer"
            || !(1..=900).contains(&self.expires_in)
        {
            return Err(SessionError::InvalidResponse);
        }
        Ok(())
    }
}
impl Drop for TokenPair {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Ready,
    Blocked,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u8,
    scope: Scope,
    phase: Phase,
    refresh_token: String,
}
impl Drop for Saved {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
    }
}
struct Access {
    token: Zeroizing<String>,
    deadline: Instant,
}

trait AuthApi {
    fn challenge(&self) -> Result<Challenge, SessionError>;
    fn exchange(&mut self, challenge: Uuid, id_token: &str) -> Result<TokenPair, SessionError>;
    fn refresh(&mut self, refresh_token: &str) -> Result<TokenPair, SessionError>;
    fn identity(&self, access_token: &str) -> Result<Scope, SessionError>;
    fn logout(&mut self, refresh_token: &str) -> Result<(), SessionError>;
}
struct Engine<V: Vault, A: AuthApi> {
    scope: Scope,
    vault: V,
    api: A,
    saved: Option<Saved>,
    access: Option<Access>,
}
impl<V: Vault, A: AuthApi> Engine<V, A> {
    fn from_parts(scope: Scope, vault: V, api: A) -> Result<Self, SessionError> {
        let saved = if let Some(raw) = vault.load()? {
            let raw = Zeroizing::new(raw);
            let value: Saved =
                serde_json::from_str(&raw).map_err(|_| SessionError::CredentialStore)?;
            if value.version != 1 || !valid_token(&value.refresh_token) {
                return Err(SessionError::CredentialStore);
            }
            if value.scope != scope {
                return Err(SessionError::IdentityMismatch);
            }
            Some(value)
        } else {
            None
        };
        Ok(Self {
            scope,
            vault,
            api,
            saved,
            access: None,
        })
    }
    fn persist(&self) -> Result<(), SessionError> {
        let value = self.saved.as_ref().ok_or(SessionError::SignInRequired)?;
        let raw = Zeroizing::new(
            serde_json::to_string(value).map_err(|_| SessionError::CredentialStore)?,
        );
        self.vault.save(&raw)
    }
    fn block(&mut self) -> Result<(), SessionError> {
        self.access = None;
        if let Some(saved) = &mut self.saved {
            saved.phase = Phase::Blocked;
            self.persist()?;
        }
        Ok(())
    }
    fn accept(&mut self, mut pair: TokenPair, started: Instant) -> Result<(), SessionError> {
        let validation = pair.validate();
        if !valid_token(&pair.refresh_token) {
            return Err(SessionError::InvalidResponse);
        }
        self.saved = Some(Saved {
            version: 1,
            scope: self.scope.clone(),
            phase: Phase::Blocked,
            refresh_token: std::mem::take(&mut pair.refresh_token),
        });
        // A crash during identity checks must leave a revocation-only credential.
        if self.persist().is_err() {
            let _ = self.api.logout(&self.saved.as_ref().unwrap().refresh_token);
            return Err(SessionError::CredentialStore);
        }
        if let Err(error) = validation {
            let _ = self.api.logout(&self.saved.as_ref().unwrap().refresh_token);
            return Err(error);
        }
        if self.api.identity(&pair.access_token)? != self.scope {
            return Err(SessionError::IdentityMismatch);
        }
        let deadline = started + Duration::from_secs(pair.expires_in.saturating_sub(30));
        if deadline <= Instant::now() {
            return Err(SessionError::SignInRequired);
        }
        self.saved.as_mut().unwrap().phase = Phase::Ready;
        if let Err(error) = self.persist() {
            self.saved.as_mut().unwrap().phase = Phase::Blocked;
            return Err(error);
        }
        self.access = Some(Access {
            token: Zeroizing::new(std::mem::take(&mut pair.access_token)),
            deadline,
        });
        Ok(())
    }
    fn sign_in(&mut self, challenge: Uuid, id_token: &str) -> Result<(), SessionError> {
        if self.saved.is_some() {
            return Err(SessionError::AlreadySignedIn);
        }
        if id_token.is_empty() || id_token.len() > 12288 {
            return Err(SessionError::InvalidConfiguration);
        }
        let started = Instant::now();
        let pair = self.api.exchange(challenge, id_token)?;
        self.accept(pair, started)
    }
    fn access_token(&mut self) -> Result<&str, SessionError> {
        if self.saved.as_ref().is_none_or(|s| s.phase != Phase::Ready) {
            return Err(SessionError::SignInRequired);
        }
        if self
            .access
            .as_ref()
            .is_none_or(|a| a.deadline <= Instant::now())
        {
            // Persist before sending. A lost refresh response must never cause token reuse.
            self.block()?;
            let started = Instant::now();
            let pair = self
                .api
                .refresh(&self.saved.as_ref().unwrap().refresh_token)?;
            self.accept(pair, started)?;
        }
        Ok(&self.access.as_ref().unwrap().token)
    }
    fn sign_out(&mut self) -> Result<(), SessionError> {
        let persisted = self.block();
        if let Some(saved) = &self.saved
            && self.api.logout(&saved.refresh_token).is_err()
        {
            persisted?;
            return Err(SessionError::RevocationPending);
        }
        self.vault.delete()?;
        self.saved = None;
        Ok(())
    }
    fn action<T>(
        &mut self,
        mut operation: impl FnMut(&DevelopmentClient) -> Result<T, SyncError>,
    ) -> Result<T, SessionError> {
        for attempt in 0..2 {
            let scope = self.scope.clone();
            let client =
                DevelopmentClient::new(scope, self.access_token()?).map_err(SessionError::Sync)?;
            match operation(&client) {
                Err(SyncError::Authentication) if attempt == 0 => self.access = None,
                Err(SyncError::Authentication) => {
                    self.block()?;
                    return Err(SessionError::SignInRequired);
                }
                Err(SyncError::IdentityMismatch) => {
                    self.block()?;
                    return Err(SessionError::IdentityMismatch);
                }
                other => return other.map_err(SessionError::Sync),
            }
        }
        unreachable!()
    }
}

/// A blocking, account-bound session client. Use it only on a worker thread.
/// One instance owns the account credential until dropped. Dropping does not sign out.
pub struct SessionClient {
    engine: Engine<JournalVault<PlatformVault>, LocalAuthApi>,
    _lock: File,
}
impl SessionClient {
    /// Opens the platform credential store for this loopback API and account.
    /// This method does not contact the API or renew a stored session.
    pub fn local(scope: Scope) -> Result<Self, SessionError> {
        let api = LocalAuthApi::new(scope.clone())?;
        let (vault, lock) = PlatformVault::open(&scope)?;
        Ok(Self {
            engine: Engine::from_parts(scope, vault, api)?,
            _lock: lock,
        })
    }
    /// Creates a nonce for the caller's Google browser flow with state and PKCE.
    pub fn challenge(&self) -> Result<Challenge, SessionError> {
        self.engine.api.challenge()
    }
    /// Exchanges the Google ID token and checks the expected tenant and person before saving access.
    /// The caller must validate its browser callback state. Google credentials are not retained.
    pub fn sign_in(&mut self, challenge: Uuid, id_token: &str) -> Result<(), SessionError> {
        self.engine.sign_in(challenge, id_token)
    }
    /// Blocks local use, revokes the remote session, and removes its stored credential.
    /// Retry this method after a failure. It does not remove queued time entries.
    pub fn sign_out(&mut self) -> Result<(), SessionError> {
        self.engine.sign_out()
    }
    /// Checks access and renews when necessary. It does not send time entries.
    pub fn check_identity(&mut self) -> Result<(), SessionError> {
        self.engine.action(|client| client.check_identity())
    }
    pub fn sync(&mut self, queue: &mut Queue) -> Result<usize, SessionError> {
        self.sync_batch(queue, usize::MAX)
    }
    /// Sends at most `limit` operations, including requests after session renewal.
    pub fn sync_batch(&mut self, queue: &mut Queue, limit: usize) -> Result<usize, SessionError> {
        if limit == 0 {
            return Err(SessionError::InvalidConfiguration);
        }
        if queue.scope != self.engine.scope {
            return Err(SessionError::IdentityMismatch);
        }
        let before = queue
            .pending()
            .map_err(|_| SessionError::Sync(SyncError::Storage))?
            .len();
        self.engine.action(|client| {
            let current = queue.pending().map_err(|_| SyncError::Storage)?.len();
            let remaining = limit.saturating_sub(before - current);
            if remaining == 0 {
                Ok(0)
            } else {
                client.sync_batch(queue, remaining)
            }
        })?;
        let after = queue
            .pending()
            .map_err(|_| SessionError::Sync(SyncError::Storage))?
            .len();
        Ok(before - after)
    }
    pub fn read(&mut self, id: Uuid) -> Result<RemoteEntry, SessionError> {
        self.engine.action(|client| client.read(id))
    }
    pub fn list_entries(
        &mut self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
        limit: u16,
        cursor: Option<Uuid>,
    ) -> Result<Page<RemoteEntry>, SessionError> {
        self.engine
            .action(|client| client.list_entries(from, to, limit, cursor))
    }
    pub fn list_projects(
        &mut self,
        limit: u16,
        cursor: Option<Uuid>,
    ) -> Result<Page<Project>, SessionError> {
        self.engine
            .action(|client| client.list_projects(limit, cursor))
    }
    pub fn pull_changes(&mut self, queue: &mut Queue) -> Result<bool, SessionError> {
        if queue.scope != self.engine.scope {
            return Err(SessionError::IdentityMismatch);
        }
        self.engine.action(|client| client.pull_changes(queue))
    }
}
