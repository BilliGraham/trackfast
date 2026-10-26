mod entries;
mod operations;
mod settings;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_postgres::{Client, Config, NoTls};
use uuid::Uuid;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_entries.sql"),
    include_str!("../migrations/002_settings.sql"),
];

#[derive(Clone, Copy)]
struct Actor {
    tenant: Uuid,
    person: Uuid,
}

#[derive(Clone)]
pub struct AppState {
    config: Config,
    credentials: Arc<HashMap<[u8; 32], Actor>>,
}

impl AppState {
    /// Creates a development adapter with fixed identities and personal settings.
    /// The caller must bind its HTTP listener to loopback only.
    pub fn development(mut config: Config, credentials: Vec<(String, Uuid, Uuid)>) -> Self {
        config.connect_timeout(Duration::from_secs(5));
        let credentials = credentials
            .into_iter()
            .map(|(token, tenant, person)| {
                assert!(
                    token.len() >= 32,
                    "Development tokens require at least 32 characters."
                );
                (
                    Sha256::digest(token.as_bytes()).into(),
                    Actor { tenant, person },
                )
            })
            .collect();
        Self {
            config,
            credentials: Arc::new(credentials),
        }
    }

    async fn connect(&self) -> Result<Client, ApiError> {
        let (client, connection) = self.config.connect(NoTls).await.map_err(internal)?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .batch_execute("SET statement_timeout = '10s'; SET lock_timeout = '5s'")
            .await
            .map_err(internal)?;
        Ok(client)
    }

    pub async fn migrate(&self) -> Result<(), ApiError> {
        let mut client = self.connect().await?;
        let tx = client.transaction().await.map_err(internal)?;
        tx.query_one("SELECT pg_advisory_xact_lock(81425631)", &[])
            .await
            .map_err(internal)?;
        tx.batch_execute("CREATE TABLE IF NOT EXISTS api_migrations (version integer PRIMARY KEY)")
            .await
            .map_err(internal)?;
        let version: i32 = tx
            .query_one("SELECT coalesce(max(version),0) FROM api_migrations", &[])
            .await
            .map_err(internal)?
            .get(0);
        if version < 0 || version as usize > MIGRATIONS.len() {
            return Err(internal(()));
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.batch_execute(sql).await.map_err(internal)?;
            tx.execute(
                "INSERT INTO api_migrations(version) VALUES ($1)",
                &[&((index + 1) as i32)],
            )
            .await
            .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)
    }

    fn actor(&self, headers: &HeaderMap) -> Result<Actor, ApiError> {
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(unauthenticated)?;
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        self.credentials
            .get(&digest)
            .copied()
            .ok_or_else(unauthenticated)
    }
}

#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.0,
            Json(json!({"code":self.1,"message":self.2,"request_id":Uuid::new_v4()})),
        )
            .into_response();
        if self.0 == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert("www-authenticate", "Bearer".parse().unwrap());
        }
        response
    }
}

fn internal<E>(_: E) -> ApiError {
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "The request could not be completed.",
    )
}
fn unauthenticated() -> ApiError {
    ApiError(
        StatusCode::UNAUTHORIZED,
        "unauthenticated",
        "A valid access token is required.",
    )
}
fn invalid() -> ApiError {
    ApiError(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Check the entry fields. This slice accepts unassigned duration entries only.",
    )
}
fn missing() -> ApiError {
    ApiError(
        StatusCode::NOT_FOUND,
        "not_found",
        "The entry is unavailable.",
    )
}
fn bad_request() -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        "bad_request",
        "Check the request and operation key.",
    )
}

#[derive(Serialize, Deserialize)]
struct Reply {
    status: u16,
    body: Option<Value>,
    etag: Option<String>,
    location: Option<String>,
}

impl IntoResponse for Reply {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = match self.body {
            Some(body) => (status, Json(body)).into_response(),
            None => status.into_response(),
        };
        if let Some(etag) = self.etag {
            response.headers_mut().insert("etag", etag.parse().unwrap());
        }
        if let Some(location) = self.location {
            response
                .headers_mut()
                .insert("location", location.parse().unwrap());
        }
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
        response
    }
}

async fn identity(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    Ok(Reply {
        status: 200,
        body: Some(json!({"tenant_id":actor.tenant,"person_id":actor.person})),
        etag: None,
        location: None,
    })
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/me", get(identity))
        .route(
            "/v1/me/settings",
            get(settings::read).put(settings::replace),
        )
        .route("/v1/me/entries", post(entries::create).get(entries::list))
        .route(
            "/v1/me/entries/{entry_id}",
            get(entries::read)
                .put(entries::replace)
                .delete(entries::delete),
        )
        .layer(DefaultBodyLimit::max(16_384))
        .with_state(state)
}
