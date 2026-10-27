#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
use crate::{Actor, ApiError, AppState, Reply, bad_request, internal, unauthenticated};
use axum::{Json, extract::State, http::StatusCode};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct KeyCache {
    keys: Vec<(String, DecodingKey)>,
    expires: Instant,
    retry_after: Instant,
}
pub(crate) struct Google {
    client_id: String,
    http: reqwest::Client,
    cache: Mutex<KeyCache>,
}

#[derive(Deserialize, Clone)]
struct Claims {
    sub: String,
    aud: String,
    azp: Option<String>,
    iat: u64,
    nonce: String,
    email_verified: bool,
}

impl Google {
    fn new(client_id: String) -> Result<Self, ApiError> {
        if client_id.is_empty() || client_id.len() > 255 {
            return Err(bad_request());
        }
        Ok(Self {
            client_id,
            http: reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(internal)?,
            cache: Mutex::new(KeyCache {
                keys: vec![],
                expires: Instant::now(),
                retry_after: Instant::now(),
            }),
        })
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, ApiError> {
        let mut cache = self.cache.lock().await;
        let now = Instant::now();
        if cache.expires > now
            && let Some((_, key)) = cache.keys.iter().find(|(id, _)| id == kid)
        {
            return Ok(key.clone());
        }
        if cache.retry_after > now {
            return Err(unauthenticated());
        }
        cache.retry_after = now + Duration::from_secs(30);
        let mut response = self
            .http
            .get("https://www.googleapis.com/oauth2/v3/certs")
            .send()
            .await
            .map_err(|_| unauthenticated())?
            .error_for_status()
            .map_err(|_| unauthenticated())?;
        let max_age = response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(',')
                    .find_map(|s| s.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
            })
            .unwrap_or(300)
            .min(3600);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unauthenticated())? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(unauthenticated());
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks: JwkSet = serde_json::from_slice(&bytes).map_err(|_| unauthenticated())?;
        cache.keys = jwks
            .keys
            .iter()
            .filter_map(|jwk| Some((jwk.common.key_id.clone()?, DecodingKey::from_jwk(jwk).ok()?)))
            .collect();
        cache.expires = Instant::now() + Duration::from_secs(max_age);
        cache
            .keys
            .iter()
            .find(|(id, _)| id == kid)
            .map(|(_, k)| k.clone())
            .ok_or_else(unauthenticated)
    }

    async fn verify(&self, token: &str, nonce: &str) -> Result<String, ApiError> {
        if token.len() > 12288 {
            return Err(unauthenticated());
        }
        let header = decode_header(token).map_err(|_| unauthenticated())?;
        if header.alg != Algorithm::RS256 {
            return Err(unauthenticated());
        }
        let kid = header.kid.ok_or_else(unauthenticated)?;
        let key = self.key(&kid).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.client_id]);
        validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 0;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| unauthenticated())?
            .claims;
        if claims.aud != self.client_id
            || claims.azp.as_ref().is_some_and(|v| v != &self.client_id)
            || claims.iat > jsonwebtoken::get_current_timestamp()
            || claims.nonce != nonce
            || !claims.email_verified
            || claims.sub.is_empty()
            || claims.sub.len() > 255
        {
            return Err(unauthenticated());
        }
        Ok(claims.sub)
    }
}

fn secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
fn reply(body: Option<Value>) -> Reply {
    Reply {
        status: if body.is_some() { 200 } else { 204 },
        body,
        etag: None,
        location: None,
    }
}
fn tokens(access: &str, refresh: &str, expires_in: i64) -> Reply {
    reply(Some(
        json!({"access_token":access,"refresh_token":refresh,"token_type":"Bearer","expires_in":expires_in}),
    ))
}
fn google(state: &AppState) -> Result<&Google, ApiError> {
    state.google.as_deref().ok_or_else(unauthenticated)
}

impl AppState {
    /// Creates a Google identity adapter. Bind the pilot listener to loopback only.
    pub fn google(config: tokio_postgres::Config, client_id: String) -> Result<Self, ApiError> {
        let mut state = Self::development(config, vec![]);
        state.google = Some(Arc::new(Google::new(client_id)?));
        Ok(state)
    }

    /// Adds an explicitly approved Google subject. Existing roles and disabled records remain unchanged.
    pub async fn provision_google_member(
        &self,
        subject: &str,
        tenant: Uuid,
        person: Uuid,
        administrator: bool,
    ) -> Result<(), ApiError> {
        google(self)?;
        if subject.is_empty() || subject.len() > 255 {
            return Err(bad_request());
        }
        let client = self.connect().await?;
        client.execute("INSERT INTO api_auth_members(subject,tenant_id,person_id,administrator) VALUES ($1,$2,$3,$4) ON CONFLICT(subject) DO NOTHING", &[&subject,&tenant,&person,&administrator]).await.map_err(internal)?;
        let row = client
            .query_one(
                "SELECT tenant_id,person_id FROM api_auth_members WHERE subject=$1",
                &[&subject],
            )
            .await
            .map_err(internal)?;
        if row.get::<_, Uuid>(0) != tenant || row.get::<_, Uuid>(1) != person {
            return Err(bad_request());
        }
        Ok(())
    }

    /// Disables a member and revokes all sessions. This operation does not delete time records.
    pub async fn disable_google_member(&self, subject: &str) -> Result<(), ApiError> {
        google(self)?;
        let mut client = self.connect().await?;
        let tx = client.transaction().await.map_err(internal)?;
        tx.execute(
            "UPDATE api_auth_members SET enabled=false WHERE subject=$1",
            &[&subject],
        )
        .await
        .map_err(internal)?;
        tx.execute(
            "UPDATE api_auth_sessions SET revoked=true WHERE subject=$1",
            &[&subject],
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    pub(crate) async fn session_actor(&self, token: &str) -> Result<(Actor, bool), ApiError> {
        if token.len() != 64 {
            return Err(unauthenticated());
        }
        let client = self.connect().await?;
        let row = client.query_opt("SELECT m.tenant_id,m.person_id,m.administrator FROM api_auth_sessions s JOIN api_auth_members m USING(subject) WHERE s.access_hash=$1 AND s.access_expires_at>now() AND s.expires_at>now() AND NOT s.revoked AND m.enabled", &[&hash(token)]).await.map_err(internal)?.ok_or_else(unauthenticated)?;
        Ok((
            Actor {
                tenant: row.get(0),
                person: row.get(1),
            },
            row.get(2),
        ))
    }
}

pub(crate) async fn challenge(State(state): State<AppState>) -> Result<Reply, ApiError> {
    google(&state)?;
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    tx.query_one("SELECT pg_advisory_xact_lock(81425632)", &[])
        .await
        .map_err(internal)?;
    tx.execute(
        "DELETE FROM api_auth_challenges WHERE expires_at<=now()",
        &[],
    )
    .await
    .map_err(internal)?;
    let count: i64 = tx
        .query_one("SELECT count(*) FROM api_auth_challenges", &[])
        .await
        .map_err(internal)?
        .get(0);
    if count >= 256 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Retry sign-in later.",
        ));
    }
    let id = Uuid::new_v4();
    let nonce = secret();
    tx.execute(
        "INSERT INTO api_auth_challenges(challenge_id,nonce) VALUES ($1,$2)",
        &[&id, &nonce],
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(reply(Some(
        json!({"challenge_id":id,"nonce":nonce,"expires_in":300}),
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    challenge_id: Uuid,
    id_token: String,
}

pub(crate) async fn exchange(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Reply, ApiError> {
    let verifier = google(&state)?;
    let body: Exchange = serde_json::from_value(body).map_err(|_| bad_request())?;
    let mut client = state.connect().await?;
    let row = client.query_opt("DELETE FROM api_auth_challenges WHERE challenge_id=$1 AND expires_at>now() RETURNING nonce",&[&body.challenge_id]).await.map_err(internal)?.ok_or_else(unauthenticated)?;
    let nonce: String = row.get(0);
    let subject = verifier.verify(&body.id_token, &nonce).await?;
    let tx = client.transaction().await.map_err(internal)?;
    tx.query_opt(
        "SELECT subject FROM api_auth_members WHERE subject=$1 AND enabled FOR SHARE",
        &[&subject],
    )
    .await
    .map_err(internal)?
    .ok_or_else(unauthenticated)?;
    tx.execute("DELETE FROM api_auth_sessions WHERE expires_at<=now()", &[])
        .await
        .map_err(internal)?;
    let id = Uuid::new_v4();
    let access = secret();
    let refresh = secret();
    tx.execute(
        "INSERT INTO api_auth_sessions(session_id,subject,access_hash) VALUES ($1,$2,$3)",
        &[&id, &subject, &hash(&access)],
    )
    .await
    .map_err(internal)?;
    tx.execute(
        "INSERT INTO api_auth_refresh(token_hash,session_id) VALUES ($1,$2)",
        &[&hash(&refresh), &id],
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(tokens(&access, &refresh, 900))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    refresh_token: String,
}

async fn rotate(state: AppState, body: Value, logout: bool) -> Result<Reply, ApiError> {
    google(&state)?;
    let body: Refresh = serde_json::from_value(body).map_err(|_| bad_request())?;
    if body.refresh_token.len() != 64 {
        return Err(unauthenticated());
    }
    let digest = hash(&body.refresh_token);
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    // Lock membership before sessions, as the disable operation does.
    let row = tx.query_opt("SELECT m.subject FROM api_auth_members m JOIN api_auth_sessions s USING(subject) JOIN api_auth_refresh r USING(session_id) WHERE r.token_hash=$1 AND m.enabled FOR SHARE OF m",&[&digest]).await.map_err(internal)?.ok_or_else(unauthenticated)?;
    let _: String = row.get(0);
    let row = tx.query_opt("SELECT s.session_id,s.revoked,s.expires_at>now(),r.used FROM api_auth_sessions s JOIN api_auth_refresh r USING(session_id) WHERE r.token_hash=$1 FOR UPDATE OF s,r",&[&digest]).await.map_err(internal)?.ok_or_else(unauthenticated)?;
    let id: Uuid = row.get(0);
    if logout || row.get::<_, bool>(3) {
        tx.execute(
            "UPDATE api_auth_sessions SET revoked=true WHERE session_id=$1",
            &[&id],
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        return if logout {
            Ok(reply(None))
        } else {
            Err(unauthenticated())
        };
    }
    if row.get::<_, bool>(1) || !row.get::<_, bool>(2) {
        return Err(unauthenticated());
    }
    let access = secret();
    let refresh = secret();
    tx.execute(
        "UPDATE api_auth_refresh SET used=true WHERE token_hash=$1",
        &[&digest],
    )
    .await
    .map_err(internal)?;
    let remaining: i64 = tx.query_one("UPDATE api_auth_sessions SET access_hash=$2,access_expires_at=LEAST(expires_at,now()+interval '15 minutes') WHERE session_id=$1 RETURNING floor(extract(epoch FROM access_expires_at-now()))::bigint",&[&id,&hash(&access)]).await.map_err(internal)?.get(0);
    tx.execute(
        "INSERT INTO api_auth_refresh(token_hash,session_id) VALUES ($1,$2)",
        &[&hash(&refresh), &id],
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(tokens(&access, &refresh, remaining))
}
pub(crate) async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Reply, ApiError> {
    rotate(state, body, false).await
}
pub(crate) async fn logout(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Reply, ApiError> {
    rotate(state, body, true).await
}
