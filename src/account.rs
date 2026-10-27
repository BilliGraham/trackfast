#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_requires_one_state_code_host_and_local_path() {
        let valid = "GET /google/callback?state=expected&code=code%2Bvalue HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
        assert_eq!(
            callback(valid, "127.0.0.1:1234", "expected").unwrap(),
            "code+value"
        );
        for bad in [
            valid.replace("state=expected", "state=wrong"),
            valid.replace("state=expected", "state=expected&state=expected"),
            valid.replace("code=code%2Bvalue", "code=one&code=two"),
            valid.replace("code=code%2Bvalue", "error=access_denied"),
            valid.replace("Host: 127.0.0.1:1234", "Host: evil.example"),
            valid.replace(
                "Host: 127.0.0.1:1234",
                "Host: 127.0.0.1:1234\r\nHost: 127.0.0.1:1234",
            ),
            valid.replace("GET /google", "POST /google"),
            valid.replace("/google/callback", "https://evil.example/google/callback"),
        ] {
            assert!(callback(&bad, "127.0.0.1:1234", "expected").is_err());
        }
    }

    #[test]
    fn authorization_uses_pkce_nonce_and_only_identity_scopes() {
        let url = authorization_url(
            "test.apps.googleusercontent.com",
            "http://127.0.0.1:1234/google/callback",
            "state",
            "nonce",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        );
        let parsed = url::Url::parse(&url).unwrap();
        let fields: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(parsed.host_str(), Some("accounts.google.com"));
        assert_eq!(
            fields["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(fields["code_challenge_method"], "S256");
        assert_eq!(fields["nonce"], "nonce");
        assert_eq!(fields["scope"], "openid email");
        assert!(!fields.contains_key("access_type"));
    }

    #[test]
    fn public_configuration_rejects_secrets_and_remote_api_addresses() {
        let value = serde_json::json!({"server":"http://127.0.0.1:8788","tenant_id":uuid::Uuid::new_v4(),"person_id":uuid::Uuid::new_v4(),"google_client_id":"test.apps.googleusercontent.com"});
        assert!(Config::parse(&value.to_string()).is_ok());
        let mut bad = value.clone();
        bad["client_secret"] = serde_json::json!("private");
        assert!(Config::parse(&bad.to_string()).is_err());
        let mut bad = value;
        bad["server"] = serde_json::json!("http://example.com:8788");
        assert!(Config::parse(&bad.to_string()).is_err());
    }
}
mod oauth;
use eframe::egui;
#[cfg(test)]
use oauth::{authorization_url, callback};
use serde::Deserialize;
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use trackfast_sync::{
    Scope,
    session::{SessionClient, SessionError},
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    server: String,
    tenant_id: uuid::Uuid,
    person_id: uuid::Uuid,
    google_client_id: String,
}
impl Config {
    fn parse(raw: &str) -> Result<Self, &'static str> {
        let config: Self =
            serde_json::from_str(raw).map_err(|_| "The pilot account configuration is invalid.")?;
        if raw.len() > 4096
            || config.tenant_id.is_nil()
            || config.person_id.is_nil()
            || !config
                .google_client_id
                .ends_with(".apps.googleusercontent.com")
            || config.google_client_id.len() > 256
            || !config
                .google_client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || trackfast_sync::transport::DevelopmentClient::new(
                config.scope(),
                "configuration-validation-only-token",
            )
            .is_err()
        {
            return Err("Check the local API address, account IDs, and Google desktop client ID.");
        }
        Ok(config)
    }
    fn scope(&self) -> Scope {
        Scope {
            server: self.server.clone(),
            tenant_id: self.tenant_id,
            person_id: self.person_id,
        }
    }
    fn load() -> Result<Option<Self>, &'static str> {
        let Some(path) = std::env::var_os("TRACKFAST_ACCOUNT_CONFIG") else {
            return Ok(None);
        };
        let mut raw = String::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(4097).read_to_string(&mut raw))
            .map_err(|_| "Trackfast cannot read the pilot account configuration.")?;
        Self::parse(&raw).map(Some)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unconfigured,
    Checking,
    SignedOut,
    SigningIn,
    Connected,
    SigningOut,
    Failed,
}
impl Status {
    pub fn busy(self) -> bool {
        matches!(self, Self::Checking | Self::SigningIn | Self::SigningOut)
    }
}
#[derive(Default)]
pub struct View {
    pub status: Status,
    pub problem: Option<String>,
}
pub enum Command {
    SignIn(Zeroizing<String>),
    Restore,
    SignOut,
    Stop,
}
pub enum Event {
    State(View),
    Browser(String),
}
pub struct Account {
    commands: Sender<Command>,
    events: Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}
impl Account {
    #[cfg(test)]
    pub(crate) fn for_test() -> (Self, Receiver<Command>) {
        let (commands, receive) = mpsc::channel();
        let (_, events) = mpsc::channel();
        (
            Self {
                commands,
                events,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
            receive,
        )
    }
    pub fn start(ctx: egui::Context) -> Self {
        let (commands, receive) = mpsc::channel();
        let (publish, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        std::thread::Builder::new()
            .name("account".into())
            .spawn(move || {
                let notify = |event| {
                    let _ = publish.send(event);
                    ctx.request_repaint();
                };
                run(receive, &notify, cancel);
            })
            .expect("the operating system must allow a new thread");
        Self {
            commands,
            events,
            cancelled,
        }
    }
    pub fn send(&self, command: Command) {
        self.cancelled.store(false, Ordering::Release);
        let _ = self.commands.send(command);
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn browser_allowed(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
    }
    pub fn receive(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}
impl Drop for Account {
    fn drop(&mut self) {
        self.cancel();
        let _ = self.commands.send(Command::Stop);
    }
}
fn restore(client: &mut Option<SessionClient>, config: &Config) -> Result<(), SessionError> {
    if client.is_none() {
        *client = Some(SessionClient::local(config.scope())?);
    }
    client.as_mut().unwrap().check_identity()
}
fn run(commands: Receiver<Command>, notify: &impl Fn(Event), cancel: Arc<AtomicBool>) {
    let config = match Config::load() {
        Ok(Some(config)) => config,
        result => {
            notify(Event::State(View {
                status: Status::Unconfigured,
                problem: result.err().map(str::to_owned),
            }));
            return;
        }
    };
    let state = |status, problem| notify(Event::State(View { status, problem }));
    let mut client = None;
    state(Status::Checking, None);
    let initial = restore(&mut client, &config);
    publish_result(initial, notify);
    while let Ok(command) = commands.recv() {
        match command {
            Command::Stop => return,
            Command::Restore => {
                state(Status::Checking, None);
                publish_result(restore(&mut client, &config), notify);
            }
            Command::SignOut => {
                state(Status::SigningOut, None);
                let result = (|| {
                    if client.is_none() {
                        client = Some(SessionClient::local(config.scope())?);
                    }
                    client.as_mut().unwrap().sign_out()
                })();
                match result {
                    Ok(()) => state(Status::SignedOut, None),
                    Err(error) => state(Status::Failed, Some(error.to_string())),
                }
            }
            Command::SignIn(secret) => {
                state(Status::SigningIn, None);
                let result = (|| -> Result<(), String> {
                    if client.is_none() {
                        client =
                            Some(SessionClient::local(config.scope()).map_err(|e| e.to_string())?);
                    }
                    let client = client.as_mut().unwrap();
                    let challenge = client.challenge().map_err(|e| e.to_string())?;
                    let token = oauth::authorize(
                        &config.google_client_id,
                        &secret,
                        &challenge.nonce,
                        challenge.expires_in,
                        &cancel,
                        |url| notify(Event::Browser(url)),
                    )
                    .map_err(str::to_owned)?;
                    if cancel.load(Ordering::Acquire) {
                        return Err("Sign-in cancelled.".into());
                    }
                    client
                        .sign_in(challenge.challenge_id, &token)
                        .map_err(|e| e.to_string())?;
                    if cancel.load(Ordering::Acquire) {
                        client.sign_out().map_err(|e| e.to_string())?;
                        return Err("Sign-in cancelled.".into());
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => state(Status::Connected, None),
                    Err(error) => state(Status::Failed, Some(error)),
                }
            }
        }
    }
}
fn publish_result(result: Result<(), SessionError>, notify: &impl Fn(Event)) {
    let view = match result {
        Ok(()) => View {
            status: Status::Connected,
            problem: None,
        },
        Err(SessionError::SignInRequired) => View {
            status: Status::SignedOut,
            problem: None,
        },
        Err(error) => View {
            status: Status::Failed,
            problem: Some(error.to_string()),
        },
    };
    notify(Event::State(view));
}
