use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt;
use trackfast_api::{AppState, router};
use uuid::Uuid;

struct Fixture {
    app: Router,
    state: AppState,
    config: Config,
    admin: tokio_postgres::Client,
    schema: String,
    token: String,
    other: String,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("TRACKFAST_TEST_DATABASE_URL")
            .expect("Set TRACKFAST_TEST_DATABASE_URL to an isolated Postgres database.");
        let mut config: Config = url.parse().unwrap();
        let (admin, connection) = config.connect(NoTls).await.unwrap();
        tokio::spawn(async move {
            connection.await.unwrap();
        });
        let schema = format!("test_{}", Uuid::new_v4().simple());
        admin
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        config.options(format!("-c search_path={schema}"));
        let token = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let other = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let tenant = Uuid::new_v4();
        let state = AppState::development(
            config.clone(),
            vec![
                (token.clone(), tenant, Uuid::new_v4()),
                (other.clone(), tenant, Uuid::new_v4()),
            ],
        );
        state.migrate().await.unwrap();
        Self {
            app: router(state.clone()),
            state,
            config,
            admin,
            schema,
            token,
            other,
        }
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        key: Option<Uuid>,
        etag: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value, Option<String>) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        if let Some(key) = key {
            req = req.header("idempotency-key", key.to_string());
        }
        if let Some(etag) = etag {
            req = req.header("if-match", etag);
        }
        let response = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let etag = response
            .headers()
            .get("etag")
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = to_bytes(response.into_body(), 100_000).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value, etag)
    }

    async fn count(&self, table: &str) -> i64 {
        let (client, connection) = self.config.connect(NoTls).await.unwrap();
        tokio::spawn(async move {
            connection.await.unwrap();
        });
        client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .await
            .unwrap()
            .get(0)
    }

    async fn close(self) {
        self.admin
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
    }
}

fn entry(id: Uuid) -> Value {
    json!({"entry_id":id,"kind":"duration","entry_date":"2026-10-26",
        "project_id":null,"description":"API test","original_seconds":781})
}

#[tokio::test]
async fn durable_client_recovers_after_lost_mutation_responses() {
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };
    use trackfast_sync::{DurationEntry, Queue, Scope, transport::DevelopmentClient};
    let f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id: serde_json::from_value(identity["tenant_id"].clone()).unwrap(),
        person_id: serde_json::from_value(identity["person_id"].clone()).unwrap(),
    };
    let lost = Arc::new(Mutex::new(HashSet::new()));
    let app = f.app.clone().layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let lost = lost.clone();
            async move {
                let method = req.method().to_string();
                let response = next.run(req).await;
                if method != "GET" && lost.lock().unwrap().insert(method) {
                    axum::response::IntoResponse::into_response(StatusCode::SERVICE_UNAVAILABLE)
                } else {
                    response
                }
            }
        },
    ));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let path = std::env::temp_dir().join(format!("trackfast-http-{}.db", Uuid::new_v4()));
    let token = f.token.clone();
    let other = f.other.clone();
    tokio::task::spawn_blocking(move || {
        let local = DurationEntry {
            entry_id: Uuid::new_v4(),
            entry_date: "2026-10-26".parse().unwrap(),
            description: "Recovery test".into(),
            original_seconds: 781,
        };
        let key;
        {
            let mut queue = Queue::open(&path, &scope).unwrap();
            key = queue.enqueue(&local).unwrap();
            let wrong = DevelopmentClient::new(scope.clone(), &other).unwrap();
            assert!(wrong.sync(&mut queue).is_err());
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
            let client = DevelopmentClient::new(scope.clone(), &token).unwrap();
            assert!(client.sync(&mut queue).is_err());
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
        }
        {
            let mut queue = Queue::open(&path, &scope).unwrap();
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
            let client = DevelopmentClient::new(scope.clone(), &token).unwrap();
            assert_eq!(client.sync(&mut queue).unwrap(), 1);
            assert!(queue.pending().unwrap().is_empty());
            assert_eq!(queue.entry(local.entry_id).unwrap(), Some(local.clone()));
            let changed = DurationEntry {
                original_seconds: 10800,
                ..local.clone()
            };
            let edit = queue.replace(&changed).unwrap();
            assert!(client.sync(&mut queue).is_err());
            drop(queue);
            let mut queue = Queue::open(&path, &scope).unwrap();
            assert_eq!(queue.pending().unwrap()[0].operation_id, edit);
            assert_eq!(client.sync(&mut queue).unwrap(), 1);
            let deletion = queue.delete(local.entry_id).unwrap();
            assert!(client.sync(&mut queue).is_err());
            drop(queue);
            let mut queue = Queue::open(&path, &scope).unwrap();
            assert_eq!(queue.pending().unwrap()[0].operation_id, deletion);
            assert_eq!(client.sync(&mut queue).unwrap(), 1);
            assert_eq!(queue.entry(local.entry_id).unwrap(), None);
        }
        std::fs::remove_file(path).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(f.count("api_entries").await, 1);
    assert_eq!(f.count("api_operations").await, 3);
    assert_eq!(f.count("api_audit").await, 3);
    server.abort();
    f.close().await;
}

#[tokio::test]
async fn simultaneous_retries_make_one_change() {
    let f = Fixture::new().await;
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let (first, second) = tokio::join!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id)
        ),
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id)
        )
    );
    assert_eq!(first.0, StatusCode::CREATED);
    assert_eq!(first, second);
    assert_eq!(f.count("api_audit").await, 1);
    assert_eq!(f.count("api_operations").await, 1);
    f.close().await;
}

#[tokio::test]
async fn client_conflict_keeps_local_change_and_original_operation_key() {
    use trackfast_sync::{
        DurationEntry, Queue, Scope,
        transport::{DevelopmentClient, SyncError},
    };
    let f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id: serde_json::from_value(identity["tenant_id"].clone()).unwrap(),
        person_id: serde_json::from_value(identity["person_id"].clone()).unwrap(),
    };
    let app = f.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let path = std::env::temp_dir().join(format!("trackfast-conflict-{}.db", Uuid::new_v4()));
    let local = DurationEntry {
        entry_id: Uuid::new_v4(),
        entry_date: "2026-10-26".parse().unwrap(),
        description: "Local work".into(),
        original_seconds: 781,
    };
    let token = f.token.clone();
    let (path, scope, local) = tokio::task::spawn_blocking(move || {
        let mut queue = Queue::open(&path, &scope).unwrap();
        queue.enqueue(&local).unwrap();
        DevelopmentClient::new(scope.clone(), &token)
            .unwrap()
            .sync(&mut queue)
            .unwrap();
        (path, scope, local)
    })
    .await
    .unwrap();
    let mut remote = entry(local.entry_id);
    remote.as_object_mut().unwrap().remove("entry_id");
    remote["description"] = json!("Changed on another client");
    assert_eq!(
        f.send(
            "PUT",
            &format!("/v1/me/entries/{}", local.entry_id),
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            remote
        )
        .await
        .0,
        200
    );
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let changed = DurationEntry {
            original_seconds: 10800,
            ..local
        };
        let key;
        {
            let mut queue = Queue::open(&path, &scope).unwrap();
            key = queue.replace(&changed).unwrap();
            assert!(matches!(
                DevelopmentClient::new(scope.clone(), &token)
                    .unwrap()
                    .sync(&mut queue),
                Err(SyncError::Conflict)
            ));
        }
        {
            let mut queue = Queue::open(&path, &scope).unwrap();
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
            assert_eq!(
                queue.entry(changed.entry_id).unwrap(),
                Some(changed.clone())
            );
            assert!(matches!(
                DevelopmentClient::new(scope.clone(), &token)
                    .unwrap()
                    .sync(&mut queue),
                Err(SyncError::Conflict)
            ));
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
            let client = DevelopmentClient::new(scope, &token).unwrap();
            let remote = client.read(changed.entry_id).unwrap();
            assert_eq!(remote.version, 2);
            assert_eq!(remote.entry.description, "Changed on another client");
            let resolved = queue.resolve_with_local(key, remote.version).unwrap();
            assert_ne!(key, resolved);
            assert_eq!(client.sync(&mut queue).unwrap(), 1);
            assert_eq!(client.read(changed.entry_id).unwrap().entry, changed);
        }
        std::fs::remove_file(path).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(f.count("api_audit").await, 3);
    server.abort();
    f.close().await;
}

#[tokio::test]
async fn invalid_receipt_does_not_remove_local_work() {
    use trackfast_sync::{
        DurationEntry, Queue, Scope,
        transport::{DevelopmentClient, SyncError},
    };
    let f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id: serde_json::from_value(identity["tenant_id"].clone()).unwrap(),
        person_id: serde_json::from_value(identity["person_id"].clone()).unwrap(),
    };
    let app = f.app.clone().layer(axum::middleware::from_fn(
        |req: axum::extract::Request, next: axum::middleware::Next| async {
            let create = req.method() == "POST";
            let response = next.run(req).await;
            if create {
                let (parts, body) = response.into_parts();
                let bytes = to_bytes(body, 32768).await.unwrap();
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["original_seconds"] = json!(1);
                axum::response::Response::from_parts(parts, Body::from(value.to_string()))
            } else {
                response
            }
        },
    ));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let path = std::env::temp_dir().join(format!("trackfast-receipt-{}.db", Uuid::new_v4()));
        {
            let mut queue = Queue::open(&path, &scope).unwrap();
            let local = DurationEntry {
                entry_id: Uuid::new_v4(),
                entry_date: "2026-10-26".parse().unwrap(),
                description: "Receipt test".into(),
                original_seconds: 781,
            };
            let key = queue.enqueue(&local).unwrap();
            assert!(matches!(
                DevelopmentClient::new(scope, &token)
                    .unwrap()
                    .sync(&mut queue),
                Err(SyncError::InvalidReceipt)
            ));
            assert_eq!(queue.pending().unwrap()[0].operation_id, key);
            assert_eq!(queue.entry(local.entry_id).unwrap(), Some(local));
        }
        std::fs::remove_file(path).unwrap();
    })
    .await
    .unwrap();
    server.abort();
    f.close().await;
}

#[tokio::test]
async fn failed_audit_rolls_back_entry_and_receipt() {
    let f = Fixture::new().await;
    f.admin
        .batch_execute(&format!(
            "ALTER TABLE {}.api_audit ADD CONSTRAINT fail_audit CHECK (false)",
            f.schema
        ))
        .await
        .unwrap();
    let result = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4()),
        )
        .await;
    assert_eq!(result.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(result.1["code"], "internal_error");
    assert!(!result.1.to_string().contains("fail_audit"));
    assert_eq!(f.count("api_entries").await, 0);
    assert_eq!(f.count("api_operations").await, 0);
    assert_eq!(f.count("api_audit").await, 0);
    f.close().await;
}

#[tokio::test]
async fn missing_precondition_and_invalid_date_ranges_are_rejected() {
    let f = Fixture::new().await;
    let id = Uuid::new_v4();
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(id)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        f.send(
            "DELETE",
            &format!("/v1/me/entries/{id}"),
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::PRECONDITION_REQUIRED
    );
    for query in [
        "from_date=2026-10-27&to_date=2026-10-26",
        "from_date=2026-1-1&to_date=2026-10-26",
    ] {
        assert_eq!(
            f.send(
                "GET",
                &format!("/v1/me/entries?{query}"),
                Some(&f.token),
                None,
                None,
                Value::Null
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    f.close().await;
}

#[tokio::test]
async fn persistent_entry_workflow_retries_conflicts_and_deletion() {
    let mut f = Fixture::new().await;
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let path = format!("/v1/me/entries/{id}");
    let create = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id),
        )
        .await;
    assert_eq!(create.0, StatusCode::CREATED);
    assert_eq!(create.1["effective_seconds"], 781);
    assert_eq!(create.1["rounding_interval_minutes"], Value::Null);
    assert_eq!(create.2.as_deref(), Some("\"1\""));
    f.app = router(f.state.clone());
    let read = f
        .send("GET", &path, Some(&f.token), None, None, Value::Null)
        .await;
    assert_eq!(read.1, create.1);
    let replay = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id),
        )
        .await;
    assert_eq!(replay, create);
    assert_eq!(f.count("api_audit").await, 1);
    let mut changed = entry(id);
    changed["original_seconds"] = json!(10800);
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            changed.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    changed.as_object_mut().unwrap().remove("entry_id");
    let update_key = Uuid::new_v4();
    let update = f
        .send(
            "PUT",
            &path,
            Some(&f.token),
            Some(update_key),
            Some("\"1\""),
            changed.clone(),
        )
        .await;
    assert_eq!(update.0, StatusCode::OK);
    assert_eq!(update.1["version"], 2);
    assert_eq!(update.1["effective_seconds"], 10800);
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(update_key),
            Some("\"1\""),
            changed.clone()
        )
        .await,
        update
    );
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            changed
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(f.count("api_audit").await, 2);
    let delete_key = Uuid::new_v4();
    let deleted = f
        .send(
            "DELETE",
            &path,
            Some(&f.token),
            Some(delete_key),
            Some("\"2\""),
            Value::Null,
        )
        .await;
    assert_eq!(deleted.0, StatusCode::NO_CONTENT);
    assert_eq!(
        f.send(
            "DELETE",
            &path,
            Some(&f.token),
            Some(delete_key),
            Some("\"2\""),
            Value::Null
        )
        .await,
        deleted
    );
    assert_eq!(
        f.send("GET", &path, Some(&f.token), None, None, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(id)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(f.count("api_entries").await, 1);
    assert_eq!(f.count("api_audit").await, 3);
    assert_eq!(f.count("api_operations").await, 3);
    f.close().await;
}

#[tokio::test]
async fn authentication_and_ownership_prevent_access_and_mutation() {
    let f = Fixture::new().await;
    let id = Uuid::new_v4();
    let path = format!("/v1/me/entries/{id}");
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            None,
            Some(Uuid::new_v4()),
            None,
            entry(id)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some("bad"),
            Some(Uuid::new_v4()),
            None,
            entry(id)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(id)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    for method in ["GET", "PUT", "DELETE"] {
        let mut body = entry(id);
        body.as_object_mut().unwrap().remove("entry_id");
        assert_eq!(
            f.send(
                method,
                &path,
                Some(&f.other),
                Some(Uuid::new_v4()),
                Some("\"1\""),
                body
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let list = f
        .send(
            "GET",
            "/v1/me/entries?from_date=2026-10-26&to_date=2026-10-26",
            Some(&f.other),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(list.0, StatusCode::OK);
    assert_eq!(list.1["items"], json!([]));
    assert_eq!(f.count("api_audit").await, 1);
    f.close().await;
}

#[tokio::test]
async fn invalid_input_does_not_write_records() {
    let f = Fixture::new().await;
    for (field, value) in [
        ("original_seconds", json!(-1)),
        ("entry_date", json!("2026-02-30")),
        ("person_id", json!(Uuid::new_v4())),
        ("effective_seconds", json!(900)),
        ("project_id", json!(Uuid::new_v4())),
    ] {
        let mut body = entry(Uuid::new_v4());
        body[field] = value;
        assert_eq!(
            f.send(
                "POST",
                "/v1/me/entries",
                Some(&f.token),
                Some(Uuid::new_v4()),
                None,
                body
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            None,
            None,
            entry(Uuid::new_v4())
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(f.count("api_entries").await, 0);
    assert_eq!(f.count("api_operations").await, 0);
    assert_eq!(f.count("api_audit").await, 0);
    f.close().await;
}
