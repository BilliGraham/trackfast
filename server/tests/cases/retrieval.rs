use super::*;

#[tokio::test]
async fn change_revisions_follow_commit_order_and_audit_failure_rolls_back() {
    let f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let tenant: Uuid = serde_json::from_value(identity["tenant_id"].clone()).unwrap();
    let person: Uuid = serde_json::from_value(identity["person_id"].clone()).unwrap();
    let initial = Uuid::new_v4();
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(initial)
        )
        .await
        .0,
        201
    );
    let (mut client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    let tx = client.transaction().await.unwrap();
    tx.query_one(
        "SELECT revision FROM api_entry_clocks WHERE tenant_id=$1 AND person_id=$2 FOR UPDATE",
        &[&tenant, &person],
    )
    .await
    .unwrap();
    let app = f.app.clone();
    let token = f.token.clone();
    let later = Uuid::new_v4();
    let request = Request::builder()
        .method("POST")
        .uri("/v1/me/entries")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", Uuid::new_v4().to_string())
        .body(Body::from(entry(later).to_string()))
        .unwrap();
    let write = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!write.is_finished());
    let page = f
        .send(
            "GET",
            "/v1/me/entry-changes",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await
        .1;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    tx.commit().await.unwrap();
    assert_eq!(write.await.unwrap().status(), 201);
    let next = f
        .send(
            "GET",
            &format!(
                "/v1/me/entry-changes?cursor={}",
                page["next_cursor"].as_str().unwrap()
            ),
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await
        .1;
    assert_eq!(next["items"][0]["entry_id"], json!(later));
    client
        .batch_execute("ALTER TABLE api_audit ADD CONSTRAINT reject_audit CHECK(false) NOT VALID")
        .await
        .unwrap();
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4())
        )
        .await
        .0,
        500
    );
    assert_eq!(f.count("api_entry_changes").await, 2);
    assert_eq!(f.count("api_entries").await, 2);
    assert_eq!(f.count("api_operations").await, 2);
    f.close().await;
}

#[tokio::test]
async fn cursors_expire_and_admin_writes_rollback_without_partial_membership() {
    let mut f = Fixture::new().await;
    let (tenant, person, _) = super::projects::seed(&f, &f.token, false).await;
    f.app = router(
        f.state
            .clone()
            .with_development_administrators(vec![(tenant, person)]),
    );
    for _ in 0..2 {
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4()),
        )
        .await;
    }
    let base = "/v1/me/entries?from_date=2026-10-26&to_date=2026-10-26&limit=1";
    let first = f
        .send("GET", base, Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let url = format!("{base}&cursor={}", first["next_cursor"].as_str().unwrap());
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client
        .batch_execute("UPDATE api_page_cursors SET expires_at=now()-interval '1 second'")
        .await
        .unwrap();
    assert_eq!(
        f.send("GET", &url, Some(&f.token), None, None, Value::Null)
            .await
            .0,
        422
    );
    let id = Uuid::new_v4();
    let body = json!({"project_id":id,"name":"Atomic project","colour":"#123456","archived":false,"member_ids":[person]});
    assert_eq!(
        f.send(
            "POST",
            "/v1/admin/projects",
            None,
            Some(Uuid::new_v4()),
            None,
            body.clone()
        )
        .await
        .0,
        401
    );
    client.batch_execute("ALTER TABLE api_project_audit ADD CONSTRAINT reject_project_audit CHECK(false) NOT VALID").await.unwrap();
    assert_eq!(
        f.send(
            "POST",
            "/v1/admin/projects",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            body
        )
        .await
        .0,
        500
    );
    assert_eq!(f.count("api_projects").await, 1);
    assert_eq!(f.count("api_project_members").await, 1);
    assert_eq!(f.count("api_project_audit").await, 0);
    assert_eq!(f.count("api_operations").await, 2);
    f.close().await;
}

#[tokio::test]
async fn timed_project_queue_recovers_and_retrieves_remote_changes_over_http() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use trackfast_sync::{Entry, Queue, Scope, Timing, transport::DevelopmentClient};
    let f = Fixture::new().await;
    let (tenant_id, person_id, project) = super::projects::seed(&f, &f.token, false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id,
        person_id,
    };
    let lost = Arc::new(AtomicBool::new(false));
    let app = f.app.clone().layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let lost = lost.clone();
            async move {
                let create = req.method() == "POST";
                let response = next.run(req).await;
                if create && response.status().is_success() && !lost.swap(true, Ordering::SeqCst) {
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
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let path = std::env::temp_dir().join(format!("timed-http-{}.db", Uuid::new_v4()));
        let mirror_path = std::env::temp_dir().join(format!("mirror-{}.db", Uuid::new_v4()));
        let client = DevelopmentClient::new(scope.clone(), &token).unwrap();
        assert_eq!(
            client.list_projects(1, None).unwrap().items[0].project_id,
            project
        );
        let mut entry = Entry {
            entry_id: Uuid::new_v4(),
            entry_date: "2026-10-26".parse().unwrap(),
            description: "Timed recovery".into(),
            original_seconds: 780,
            project_id: Some(project),
            timing: Some(Timing {
                started_at: "2026-10-26T09:03:00+02:00".into(),
                stopped_at: "2026-10-26T09:16:00+02:00".into(),
                time_zone: "Africa/Johannesburg".into(),
            }),
        };
        let mut queue = Queue::open(&path, &scope).unwrap();
        let key = queue.enqueue(&entry).unwrap();
        assert!(client.sync(&mut queue).is_err());
        drop(queue);
        let mut queue = Queue::open(&path, &scope).unwrap();
        assert_eq!(queue.pending().unwrap()[0].operation_id, key);
        assert_eq!(client.sync(&mut queue).unwrap(), 1);
        assert_eq!(client.read(entry.entry_id).unwrap().entry, entry);
        assert_eq!(
            client
                .list_entries(entry.entry_date, entry.entry_date, 1, None)
                .unwrap()
                .items[0]
                .entry,
            entry
        );
        let mut mirror = Queue::open(&mirror_path, &scope).unwrap();
        assert!(!client.pull_changes(&mut mirror).unwrap());
        assert_eq!(mirror.entry(entry.entry_id).unwrap(), Some(entry.clone()));
        entry.description = "Edited work".into();
        queue.replace(&entry).unwrap();
        client.sync(&mut queue).unwrap();
        drop(mirror);
        let mut mirror = Queue::open(&mirror_path, &scope).unwrap();
        assert!(!client.pull_changes(&mut mirror).unwrap());
        assert_eq!(mirror.entry(entry.entry_id).unwrap(), Some(entry.clone()));
        queue.delete(entry.entry_id).unwrap();
        client.sync(&mut queue).unwrap();
        assert!(!client.pull_changes(&mut mirror).unwrap());
        assert!(mirror.entry(entry.entry_id).unwrap().is_none());
        assert!(mirror.enqueue(&entry).is_err());
        drop(queue);
        drop(mirror);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(mirror_path).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(f.count("api_entries").await, 1);
    assert_eq!(f.count("api_entry_changes").await, 3);
    assert_eq!(f.count("api_audit").await, 3);
    server.abort();
    f.close().await;
}

#[tokio::test]
async fn administrators_manage_projects_with_versions_and_atomic_replays() {
    let mut f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let tenant: Uuid = serde_json::from_value(identity["tenant_id"].clone()).unwrap();
    let person: Uuid = serde_json::from_value(identity["person_id"].clone()).unwrap();
    f.app = router(
        f.state
            .clone()
            .with_development_administrators(vec![(tenant, person)]),
    );
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let mut body = json!({"project_id":id,"name":"Pilot","colour":"#aa00ff","archived":false,"member_ids":[person]});
    let created = f
        .send(
            "POST",
            "/v1/admin/projects",
            Some(&f.token),
            Some(key),
            None,
            body.clone(),
        )
        .await;
    assert_eq!(created.0, 201);
    assert_eq!(
        f.send(
            "POST",
            "/v1/admin/projects",
            Some(&f.token),
            Some(key),
            None,
            body.clone()
        )
        .await,
        created
    );
    assert_eq!(f.count("api_project_audit").await, 1);
    let path = format!("/v1/admin/projects/{id}");
    assert_eq!(
        f.send("GET", &path, Some(&f.other), None, None, Value::Null)
            .await
            .0,
        403
    );
    assert_eq!(
        f.send(
            "GET",
            "/v1/projects",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await
        .1["items"][0]["project_id"],
        json!(id)
    );
    body.as_object_mut().unwrap().remove("project_id");
    body["archived"] = json!(true);
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            body.clone()
        )
        .await
        .0,
        200
    );
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            body.clone()
        )
        .await
        .0,
        412
    );
    body["member_ids"] = json!([Uuid::new_v4()]);
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"2\""),
            body.clone()
        )
        .await
        .0,
        422
    );
    assert_eq!(f.count("api_project_audit").await, 2);
    body["member_ids"] = json!([]);
    body["archived"] = json!(false);
    assert_eq!(
        f.send(
            "PUT",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"2\""),
            body
        )
        .await
        .0,
        200
    );
    assert_eq!(
        f.send(
            "GET",
            "/v1/projects",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await
        .1["items"],
        json!([])
    );
    assert_eq!(
        f.send(
            "GET",
            "/v1/admin/projects?limit=1",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await
        .1["items"][0]["version"],
        3
    );
    f.close().await;
}

#[tokio::test]
async fn entry_pages_bind_filters_and_changes_include_deletions() {
    let f = Fixture::new().await;
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for id in ids {
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
            201
        );
    }
    let base = "/v1/me/entries?from_date=2026-10-26&to_date=2026-10-26&limit=2";
    let first = f
        .send("GET", base, Some(&f.token), None, None, Value::Null)
        .await;
    assert_eq!(first.0, 200);
    assert_eq!(first.1["items"].as_array().unwrap().len(), 2);
    let cursor = first.1["next_cursor"].as_str().unwrap();
    let url = format!("{base}&cursor={cursor}");
    let second = f
        .send("GET", &url, Some(&f.token), None, None, Value::Null)
        .await;
    assert_eq!(second.1["items"].as_array().unwrap().len(), 1);
    assert!(second.1["next_cursor"].is_null());
    for (url, token) in [
        (url.clone(), &f.other),
        (url.replace("limit=2", "limit=1"), &f.token),
        (
            url.replace("to_date=2026-10-26", "to_date=2026-10-27"),
            &f.token,
        ),
    ] {
        assert_eq!(
            f.send("GET", &url, Some(token), None, None, Value::Null)
                .await
                .0,
            422
        );
    }
    let first = f
        .send(
            "GET",
            "/v1/me/entry-changes?limit=2",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(first.0, 200);
    assert_eq!(first.1["items"].as_array().unwrap().len(), 2);
    assert_eq!(first.1["has_more"], true);
    let url = format!(
        "/v1/me/entry-changes?limit=2&cursor={}",
        first.1["next_cursor"].as_str().unwrap()
    );
    assert_eq!(
        f.send("GET", &url, Some(&f.other), None, None, Value::Null)
            .await
            .0,
        422
    );
    let second = f
        .send("GET", &url, Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let checkpoint = second["next_cursor"].as_str().unwrap();
    let id = ids[0];
    let key = Uuid::new_v4();
    for _ in 0..2 {
        assert_eq!(
            f.send(
                "DELETE",
                &format!("/v1/me/entries/{id}"),
                Some(&f.token),
                Some(key),
                Some("\"1\""),
                Value::Null
            )
            .await
            .0,
            204
        );
    }
    let changes = f
        .send(
            "GET",
            &format!("/v1/me/entry-changes?limit=2&cursor={checkpoint}"),
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await
        .1;
    assert_eq!(changes["items"].as_array().unwrap().len(), 1);
    assert_eq!(changes["items"][0]["entry_id"], json!(id));
    assert_eq!(changes["items"][0]["deleted"], true);
    assert!(changes["items"][0]["entry"].is_null());
    assert_eq!(f.count("api_entry_changes").await, 4);
    f.close().await;
}

#[tokio::test]
async fn project_administration_is_denied_by_default() {
    let f = Fixture::new().await;
    assert_eq!(f.send("POST", "/v1/admin/projects", Some(&f.token), Some(Uuid::new_v4()), None,
        json!({"project_id":Uuid::new_v4(),"name":"Pilot","colour":"#aa00ff","archived":false,"member_ids":[]})).await.0, 403);
    f.close().await;
}
