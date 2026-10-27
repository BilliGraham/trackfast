use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct StoreState {
    value: Option<String>,
    fail_save: bool,
    fail_delete: bool,
}
#[derive(Clone, Default)]
struct MemoryVault(Rc<RefCell<StoreState>>);
impl Vault for MemoryVault {
    fn load(&self) -> Result<Option<String>, SessionError> {
        Ok(self.0.borrow().value.clone())
    }
    fn save(&self, value: &str) -> Result<(), SessionError> {
        let mut state = self.0.borrow_mut();
        if state.fail_save {
            return Err(SessionError::CredentialStore);
        }
        state.value = Some(value.into());
        Ok(())
    }
    fn delete(&self) -> Result<(), SessionError> {
        let mut state = self.0.borrow_mut();
        if state.fail_delete {
            return Err(SessionError::CredentialStore);
        }
        state.value = None;
        Ok(())
    }
}
struct FakeApi {
    scope: Scope,
    vault: MemoryVault,
    refreshes: usize,
    logouts: usize,
    offline: bool,
    wrong_identity: bool,
}
fn pair() -> TokenPair {
    TokenPair {
        access_token: "a".repeat(64),
        refresh_token: "b".repeat(64),
        token_type: "Bearer".into(),
        expires_in: 900,
    }
}
impl AuthApi for FakeApi {
    fn challenge(&self) -> Result<Challenge, SessionError> {
        Ok(Challenge {
            challenge_id: Uuid::new_v4(),
            nonce: "c".repeat(64),
            expires_in: 300,
        })
    }
    fn exchange(&mut self, _: Uuid, _: &str) -> Result<TokenPair, SessionError> {
        Ok(pair())
    }
    fn refresh(&mut self, _: &str) -> Result<TokenPair, SessionError> {
        self.refreshes += 1;
        let saved: Saved =
            serde_json::from_str(self.vault.0.borrow().value.as_ref().unwrap()).unwrap();
        assert_eq!(saved.phase, Phase::Blocked);
        if self.offline {
            Err(SessionError::Unavailable)
        } else {
            Ok(pair())
        }
    }
    fn identity(&self, _: &str) -> Result<Scope, SessionError> {
        let mut scope = self.scope.clone();
        if self.wrong_identity {
            scope.person_id = Uuid::new_v4();
        }
        Ok(scope)
    }
    fn logout(&mut self, _: &str) -> Result<(), SessionError> {
        self.logouts += 1;
        if self.offline {
            Err(SessionError::Unavailable)
        } else {
            Ok(())
        }
    }
}
pub(super) fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}
fn client(scope: Scope, vault: MemoryVault) -> Engine<MemoryVault, FakeApi> {
    let api = FakeApi {
        scope: scope.clone(),
        vault: vault.clone(),
        refreshes: 0,
        logouts: 0,
        offline: false,
        wrong_identity: false,
    };
    Engine::from_parts(scope, vault, api).unwrap()
}
fn login(client: &mut Engine<MemoryVault, FakeApi>) {
    client.sign_in(Uuid::new_v4(), "identity-token").unwrap();
}

#[test]
fn restart_renews_once_and_access_tokens_never_enter_storage() {
    let vault = MemoryVault::default();
    let scope = scope();
    let mut first = client(scope.clone(), vault.clone());
    login(&mut first);
    assert!(
        !vault
            .0
            .borrow()
            .value
            .as_ref()
            .unwrap()
            .contains(&"a".repeat(64))
    );
    first.access_token().unwrap();
    assert_eq!(first.api.refreshes, 0);
    drop(first);
    let mut restored = client(scope, vault);
    restored.access_token().unwrap();
    restored.access_token().unwrap();
    assert_eq!(restored.api.refreshes, 1);
    restored.access.as_mut().unwrap().deadline = Instant::now();
    restored.access_token().unwrap();
    assert_eq!(restored.api.refreshes, 2);
}

#[test]
fn lost_refresh_response_blocks_reuse_after_restart() {
    let vault = MemoryVault::default();
    let scope = scope();
    let mut first = client(scope.clone(), vault.clone());
    login(&mut first);
    drop(first);
    let mut restored = client(scope.clone(), vault.clone());
    restored.api.offline = true;
    assert!(matches!(
        restored.access_token(),
        Err(SessionError::Unavailable)
    ));
    assert!(matches!(
        restored.access_token(),
        Err(SessionError::SignInRequired)
    ));
    assert_eq!(restored.api.refreshes, 1);
    drop(restored);
    let mut again = client(scope, vault);
    assert!(matches!(
        again.access_token(),
        Err(SessionError::SignInRequired)
    ));
    assert_eq!(again.api.refreshes, 0);
    again.sign_out().unwrap();
    assert_eq!(again.api.logouts, 1);
}

#[test]
fn offline_signout_blocks_access_and_retries_only_revocation() {
    let vault = MemoryVault::default();
    let scope = scope();
    let mut first = client(scope.clone(), vault.clone());
    login(&mut first);
    first.api.offline = true;
    assert!(matches!(
        first.sign_out(),
        Err(SessionError::RevocationPending)
    ));
    assert!(matches!(
        first.access_token(),
        Err(SessionError::SignInRequired)
    ));
    drop(first);
    let mut restored = client(scope, vault.clone());
    assert!(matches!(
        restored.access_token(),
        Err(SessionError::SignInRequired)
    ));
    restored.sign_out().unwrap();
    assert!(vault.0.borrow().value.is_none());
    assert_eq!(restored.api.refreshes, 0);
}

#[test]
fn failed_storage_never_sends_refresh_and_failed_delete_does_not_report_signout() {
    let vault = MemoryVault::default();
    let mut first = client(scope(), vault.clone());
    login(&mut first);
    first.access = None;
    vault.0.borrow_mut().fail_save = true;
    assert!(matches!(
        first.access_token(),
        Err(SessionError::CredentialStore)
    ));
    assert_eq!(first.api.refreshes, 0);
    vault.0.borrow_mut().fail_save = false;
    vault.0.borrow_mut().fail_delete = true;
    assert!(matches!(
        first.sign_out(),
        Err(SessionError::CredentialStore)
    ));
    assert!(first.access.is_none());
    vault.0.borrow_mut().fail_delete = false;
    first.sign_out().unwrap();
}

#[test]
fn identity_and_saved_scope_cannot_change_accounts() {
    let vault = MemoryVault::default();
    let mut first = client(scope(), vault.clone());
    first.api.wrong_identity = true;
    assert!(matches!(
        first.sign_in(Uuid::new_v4(), "token"),
        Err(SessionError::IdentityMismatch)
    ));
    assert!(first.access.is_none());
    first.sign_out().unwrap();
    first.api.wrong_identity = false;
    login(&mut first);
    let other = scope();
    let api = FakeApi {
        scope: other.clone(),
        vault: vault.clone(),
        refreshes: 0,
        logouts: 0,
        offline: false,
        wrong_identity: false,
    };
    assert!(matches!(
        Engine::from_parts(other, vault, api),
        Err(SessionError::IdentityMismatch)
    ));
}

#[test]
fn invalid_tokens_and_diagnostics_do_not_expose_credentials() {
    let mut tokens = pair();
    tokens.expires_in = 901;
    assert!(tokens.validate().is_err());
    tokens.expires_in = 0;
    assert!(tokens.validate().is_err());
    tokens.expires_in = 900;
    tokens.token_type = "other".into();
    assert!(tokens.validate().is_err());
    for error in [
        SessionError::CredentialStore,
        SessionError::Unavailable,
        SessionError::RevocationPending,
    ] {
        assert!(!format!("{error:?} {error}").contains(&"a".repeat(64)));
    }
}

#[test]
fn authentication_retries_once_and_other_errors_never_retry() {
    let mut first = client(scope(), MemoryVault::default());
    login(&mut first);
    let mut calls = 0;
    let result = first
        .action(|_| {
            calls += 1;
            if calls == 1 {
                Err(SyncError::Authentication)
            } else {
                Ok(42)
            }
        })
        .unwrap();
    assert_eq!(result, 42);
    assert_eq!(first.api.refreshes, 1);
    calls = 0;
    assert!(matches!(
        first.action::<()>(|_| {
            calls += 1;
            Err(SyncError::Unavailable)
        }),
        Err(SessionError::Sync(SyncError::Unavailable))
    ));
    assert_eq!(calls, 1);
    calls = 0;
    assert!(matches!(
        first.action::<()>(|_| {
            calls += 1;
            Err(SyncError::Authentication)
        }),
        Err(SessionError::SignInRequired)
    ));
    assert_eq!(calls, 2);
    assert!(matches!(
        first.access_token(),
        Err(SessionError::SignInRequired)
    ));
}

#[test]
fn corrupt_credentials_do_not_fall_back_to_an_unsigned_session() {
    let vault = MemoryVault::default();
    vault.0.borrow_mut().value = Some("not-json".into());
    let scope = scope();
    let api = FakeApi {
        scope: scope.clone(),
        vault: vault.clone(),
        refreshes: 0,
        logouts: 0,
        offline: false,
        wrong_identity: false,
    };
    assert!(matches!(
        Engine::from_parts(scope, vault, api),
        Err(SessionError::CredentialStore)
    ));
}

#[test]
fn invalid_session_response_retains_known_refresh_token_for_cleanup() {
    let vault = MemoryVault::default();
    let mut first = client(scope(), vault.clone());
    let mut tokens = pair();
    tokens.expires_in = 901;
    assert!(matches!(
        first.accept(tokens, Instant::now()),
        Err(SessionError::InvalidResponse)
    ));
    assert!(first.access.is_none());
    let stored: Saved = serde_json::from_str(vault.0.borrow().value.as_ref().unwrap()).unwrap();
    assert_eq!(stored.phase, Phase::Blocked);
    assert_eq!(first.api.logouts, 1);
    first.sign_out().unwrap();
    assert!(vault.0.borrow().value.is_none());
}

#[test]
fn signout_remains_blocked_after_restart_when_keyring_write_fails() {
    let vault = MemoryVault::default();
    let scope = scope();
    let mut first = client(scope.clone(), vault.clone());
    login(&mut first);
    drop(first);
    let marker = std::env::temp_dir().join(format!("trackfast-session-blocked-{}", Uuid::new_v4()));
    let journal = vault::JournalVault::new(vault.clone(), marker.clone());
    let api = FakeApi {
        scope: scope.clone(),
        vault: vault.clone(),
        refreshes: 0,
        logouts: 0,
        offline: true,
        wrong_identity: false,
    };
    let mut first = Engine::from_parts(scope.clone(), journal, api).unwrap();
    vault.0.borrow_mut().fail_save = true;
    assert!(matches!(
        first.sign_out(),
        Err(SessionError::CredentialStore)
    ));
    drop(first);
    vault.0.borrow_mut().fail_save = false;
    let journal = vault::JournalVault::new(vault.clone(), marker.clone());
    let api = FakeApi {
        scope: scope.clone(),
        vault: vault.clone(),
        refreshes: 0,
        logouts: 0,
        offline: false,
        wrong_identity: false,
    };
    let mut restored = Engine::from_parts(scope, journal, api).unwrap();
    assert!(matches!(
        restored.access_token(),
        Err(SessionError::SignInRequired)
    ));
    assert_eq!(restored.api.refreshes, 0);
    restored.sign_out().unwrap();
    assert!(!marker.exists());
}

fn http_server(responses: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = vec![];
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    Instant::now() < deadline,
                    "The client did not send the expected request."
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(&mut stream);
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            request.push_str(std::str::from_utf8(&bytes).unwrap());
            requests.push(request);
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nETag: \"1\"\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/forbidden\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (address, thread)
}

#[test]
fn http_signin_restore_refresh_and_signout_use_bound_identity() {
    let mut scope = scope();
    let identity =
        serde_json::json!({"tenant_id":scope.tenant_id,"person_id":scope.person_id}).to_string();
    let tokens=serde_json::json!({"access_token":"a".repeat(64),"refresh_token":"b".repeat(64),"token_type":"Bearer","expires_in":900}).to_string();
    let challenge = Uuid::new_v4();
    let challenge_body =
        serde_json::json!({"challenge_id":challenge,"nonce":"c".repeat(64),"expires_in":300})
            .to_string();
    let (address, server) = http_server(vec![
        (200, challenge_body),
        (200, tokens.clone()),
        (200, identity.clone()),
        (200, tokens),
        (200, identity),
        (204, String::new()),
    ]);
    scope.server = address;
    let vault = MemoryVault::default();
    let mut first = Engine::from_parts(
        scope.clone(),
        vault.clone(),
        LocalAuthApi::new(scope.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(first.api.challenge().unwrap().challenge_id, challenge);
    first.sign_in(challenge, "synthetic-google-token").unwrap();
    drop(first);
    let mut restored = Engine::from_parts(
        scope.clone(),
        vault.clone(),
        LocalAuthApi::new(scope).unwrap(),
    )
    .unwrap();
    restored.access_token().unwrap();
    restored.sign_out().unwrap();
    assert!(vault.0.borrow().value.is_none());
    let requests = server.join().unwrap();
    assert!(requests[1].contains("synthetic-google-token"));
    assert!(requests[3].contains("/v1/auth/refresh"));
    assert!(requests[5].contains("/v1/auth/logout"));
    assert!(
        requests[2]
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {}", "a".repeat(64)))
    );
}

#[test]
fn http_redirects_malformed_bodies_and_unapproved_destinations_are_rejected() {
    for address in [
        "https://127.0.0.1:8788",
        "http://example.com:8788",
        "http://127.0.0.1:8788/path",
        "http://name:secret@127.0.0.1:8788",
    ] {
        let mut scope = scope();
        scope.server = address.into();
        assert!(LocalAuthApi::new(scope).is_err());
    }
    for (status, body) in [
        (302, "{}".into()),
        (200, "invalid".into()),
        (200, "x".repeat(17000)),
        (401, "{}".into()),
    ] {
        let (address, server) = http_server(vec![(status, body)]);
        let mut scope = scope();
        scope.server = address;
        assert!(LocalAuthApi::new(scope).unwrap().challenge().is_err());
        server.join().unwrap();
    }
}

#[test]
fn renewal_during_queue_delivery_keeps_the_original_operation_key() {
    use serde_json::json;
    let mut scope = scope();
    let identity = json!({"tenant_id":scope.tenant_id,"person_id":scope.person_id}).to_string();
    let tokens=json!({"access_token":"a".repeat(64),"refresh_token":"b".repeat(64),"token_type":"Bearer","expires_in":900}).to_string();
    let entry = crate::Entry {
        entry_id: Uuid::new_v4(),
        entry_date: chrono::NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
        description: "Test work".into(),
        original_seconds: 900,
        project_id: None,
        timing: None,
    };
    let mut receipt = entry.request();
    receipt["person_id"] = json!(scope.person_id);
    receipt["version"] = json!(1);
    receipt["effective_seconds"] = json!(900);
    receipt["rounding_interval_minutes"] = json!(5);
    receipt["export_state"] = json!("not_exported");
    receipt["updated_at"] = json!("2026-10-27T12:00:00Z");
    let (address, server) = http_server(vec![
        (200, tokens.clone()),
        (200, identity.clone()),
        (200, identity.clone()),
        (401, "{}".into()),
        (200, tokens),
        (200, identity.clone()),
        (200, identity),
        (201, receipt.to_string()),
        (204, String::new()),
    ]);
    scope.server = address;
    let mut queue =
        Queue::prepare(rusqlite::Connection::open_in_memory().unwrap(), &scope).unwrap();
    let operation = queue.enqueue(&entry).unwrap();
    let mut client = Engine::from_parts(
        scope.clone(),
        MemoryVault::default(),
        LocalAuthApi::new(scope).unwrap(),
    )
    .unwrap();
    client
        .sign_in(Uuid::new_v4(), "synthetic-google-token")
        .unwrap();
    assert_eq!(
        client
            .action(|transport| transport.sync(&mut queue))
            .unwrap(),
        1
    );
    assert!(queue.pending().unwrap().is_empty());
    client.sign_out().unwrap();
    assert_eq!(queue.entry(entry.entry_id).unwrap(), Some(entry));
    let requests = server.join().unwrap();
    for index in [3, 7] {
        assert!(
            requests[index]
                .to_ascii_lowercase()
                .contains(&format!("idempotency-key: {operation}"))
        );
    }
    assert_eq!(
        requests[3].split("\r\n\r\n").nth(1),
        requests[7].split("\r\n\r\n").nth(1)
    );
}
