use super::*;
use serde_json::{Value, json};
use std::net::SocketAddr;

pub(super) struct LocalAuthApi {
    scope: Scope,
    agent: ureq::Agent,
}
impl LocalAuthApi {
    pub(super) fn new(scope: Scope) -> Result<Self, SessionError> {
        let address = scope
            .server
            .strip_prefix("http://")
            .and_then(|s| s.parse::<SocketAddr>().ok())
            .ok_or(SessionError::InvalidConfiguration)?;
        if !address.ip().is_loopback()
            || address.port() == 0
            || scope.server != format!("http://{address}")
            || scope.tenant_id.is_nil()
            || scope.person_id.is_nil()
        {
            return Err(SessionError::InvalidConfiguration);
        }
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Ok(Self { scope, agent })
    }
    fn post(&self, path: &str, body: Value, expected: u16) -> Result<Option<Value>, SessionError> {
        let mut response = self
            .agent
            .post(format!("{}{path}", self.scope.server))
            .send_json(body)
            .map_err(|_| SessionError::Unavailable)?;
        let status = response.status().as_u16();
        if status == 401 {
            return if path == "/v1/auth/logout" {
                Ok(None)
            } else {
                Err(SessionError::SignInRequired)
            };
        }
        if status != expected {
            return Err(SessionError::Unavailable);
        }
        if expected == 204 {
            return Ok(None);
        }
        let value = response
            .body_mut()
            .with_config()
            .limit(16384)
            .read_json()
            .map_err(|_| SessionError::InvalidResponse)?;
        Ok(Some(value))
    }
    fn tokens(&self, path: &str, body: Value) -> Result<TokenPair, SessionError> {
        let value = self
            .post(path, body, 200)?
            .ok_or(SessionError::InvalidResponse)?;
        serde_json::from_value(value).map_err(|_| SessionError::InvalidResponse)
    }
}
impl AuthApi for LocalAuthApi {
    fn challenge(&self) -> Result<Challenge, SessionError> {
        let value = self
            .post("/v1/auth/google/challenge", json!({}), 200)?
            .ok_or(SessionError::InvalidResponse)?;
        let challenge: Challenge =
            serde_json::from_value(value).map_err(|_| SessionError::InvalidResponse)?;
        if challenge.challenge_id.is_nil()
            || !valid_token(&challenge.nonce)
            || challenge.expires_in != 300
        {
            return Err(SessionError::InvalidResponse);
        }
        Ok(challenge)
    }
    fn exchange(&mut self, challenge: Uuid, id_token: &str) -> Result<TokenPair, SessionError> {
        self.tokens(
            "/v1/auth/google/exchange",
            json!({"challenge_id":challenge,"id_token":id_token}),
        )
    }
    fn refresh(&mut self, refresh_token: &str) -> Result<TokenPair, SessionError> {
        self.tokens("/v1/auth/refresh", json!({"refresh_token":refresh_token}))
    }
    fn identity(&self, access_token: &str) -> Result<Scope, SessionError> {
        let mut response = self
            .agent
            .get(format!("{}/v1/me", self.scope.server))
            .header("authorization", format!("Bearer {access_token}"))
            .call()
            .map_err(|_| SessionError::Unavailable)?;
        if response.status().as_u16() == 401 {
            return Err(SessionError::SignInRequired);
        }
        if response.status().as_u16() != 200 {
            return Err(SessionError::Unavailable);
        }
        let value: Value = response
            .body_mut()
            .with_config()
            .limit(4096)
            .read_json()
            .map_err(|_| SessionError::InvalidResponse)?;
        if value != json!({"tenant_id":self.scope.tenant_id,"person_id":self.scope.person_id}) {
            return Err(SessionError::IdentityMismatch);
        }
        Ok(self.scope.clone())
    }
    fn logout(&mut self, refresh_token: &str) -> Result<(), SessionError> {
        self.post(
            "/v1/auth/logout",
            json!({"refresh_token":refresh_token}),
            204,
        )
        .map(|_| ())
    }
}
