use super::*;

fn settings(interval: Value, target: f64, indicator: &str) -> Value {
    json!({"rounding_interval_minutes":interval,"weekly_target_hours":target,"visual_indicator":indicator})
}

#[tokio::test]
async fn migration_keeps_legacy_exact_entries_while_new_entries_use_defaults() {
    let f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let tenant: Uuid = serde_json::from_value(identity["tenant_id"].clone()).unwrap();
    let person: Uuid = serde_json::from_value(identity["person_id"].clone()).unwrap();
    let id = Uuid::new_v4();
    let mut legacy = entry(id);
    legacy["person_id"] = json!(person);
    legacy["version"] = json!(1);
    legacy["effective_seconds"] = json!(781);
    legacy["rounding_interval_minutes"] = Value::Null;
    legacy["export_state"] = json!("not_exported");
    legacy["updated_at"] = json!("2026-10-26T08:00:00Z");
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client.batch_execute("DROP TABLE api_project_cursors; DROP TABLE api_project_members; DROP TABLE api_projects; DROP TABLE api_settings_audit; DROP TABLE api_settings; DELETE FROM api_migrations WHERE version>=2;").await.unwrap();
    client.execute("INSERT INTO api_entries(tenant_id,person_id,entry_id,version,data) VALUES ($1,$2,$3,1,$4)", &[&tenant,&person,&id,&legacy]).await.unwrap();
    f.state.migrate().await.unwrap();
    f.state.migrate().await.unwrap();
    let retrieved = f
        .send(
            "GET",
            &format!("/v1/me/entries/{id}"),
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(retrieved.1, legacy);
    let mut replacement = entry(id);
    replacement.as_object_mut().unwrap().remove("entry_id");
    replacement["original_seconds"] = json!(450);
    let edited = f
        .send(
            "PUT",
            &format!("/v1/me/entries/{id}"),
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            replacement,
        )
        .await;
    assert_eq!(edited.1["effective_seconds"], 450);
    assert!(edited.1["rounding_interval_minutes"].is_null());
    let created = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4()),
        )
        .await;
    assert_eq!(created.1["effective_seconds"], 900);
    f.close().await;
}

#[tokio::test]
async fn settings_are_separate_for_the_same_person_in_different_tenants() {
    let mut f = Fixture::new().await;
    let identity = f
        .send("GET", "/v1/me", Some(&f.token), None, None, Value::Null)
        .await
        .1;
    let person: Uuid = serde_json::from_value(identity["person_id"].clone()).unwrap();
    let changed = f
        .send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            settings(Value::Null, 20.0, "glass"),
        )
        .await;
    assert_eq!(changed.0, 200);
    let original = f.app.clone();
    f.app = router(AppState::development(
        f.config.clone(),
        vec![(f.other.clone(), Uuid::new_v4(), person)],
    ));
    let other = f
        .send(
            "GET",
            "/v1/me/settings",
            Some(&f.other),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(other.1["weekly_target_hours"], 35);
    assert_eq!(other.1["rounding_interval_minutes"], 5);
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.other),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            settings(json!(5), 40.0, "progress")
        )
        .await
        .0,
        200
    );
    f.app = original;
    assert_eq!(
        f.send(
            "GET",
            "/v1/me/settings",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await,
        changed
    );
    f.close().await;
}

#[tokio::test]
async fn invalid_settings_and_missing_preconditions_do_not_write() {
    let f = Fixture::new().await;
    let valid = settings(json!(5), 35.0, "progress");
    assert_eq!(
        f.send("GET", "/v1/me/settings", None, None, None, Value::Null)
            .await
            .0,
        401
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            None,
            Some(Uuid::new_v4()),
            Some("\"1\""),
            valid.clone()
        )
        .await
        .0,
        401
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            valid.clone()
        )
        .await
        .0,
        428
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            None,
            Some("\"1\""),
            valid.clone()
        )
        .await
        .0,
        400
    );
    for (field, value) in [
        ("rounding_interval_minutes", json!(10)),
        ("rounding_interval_minutes", json!("5")),
        ("weekly_target_hours", json!(0)),
        ("weekly_target_hours", json!(-1)),
        ("weekly_target_hours", json!("35")),
        ("visual_indicator", json!("unknown")),
        ("person_id", json!(Uuid::new_v4())),
        ("version", json!(10)),
    ] {
        let mut body = valid.clone();
        body[field] = value;
        assert_eq!(
            f.send(
                "PUT",
                "/v1/me/settings",
                Some(&f.token),
                Some(Uuid::new_v4()),
                Some("\"1\""),
                body
            )
            .await
            .0,
            422,
            "{field}"
        );
    }
    for field in [
        "rounding_interval_minutes",
        "weekly_target_hours",
        "visual_indicator",
    ] {
        let mut body = valid.clone();
        body.as_object_mut().unwrap().remove(field);
        assert_eq!(
            f.send(
                "PUT",
                "/v1/me/settings",
                Some(&f.token),
                Some(Uuid::new_v4()),
                Some("\"1\""),
                body
            )
            .await
            .0,
            422
        );
    }
    for table in ["api_settings", "api_settings_audit", "api_operations"] {
        assert_eq!(f.count(table).await, 0);
    }
    f.close().await;
}

#[tokio::test]
async fn settings_retries_and_racing_updates_do_not_duplicate_or_overwrite() {
    let f = Fixture::new().await;
    let key = Uuid::new_v4();
    let body = settings(Value::Null, 32.5, "glass");
    let (a, b) = tokio::join!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(key),
            Some("\"1\""),
            body.clone()
        ),
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(key),
            Some("\"1\""),
            body
        )
    );
    assert_eq!(a.0, 200);
    assert_eq!(a, b);
    assert_eq!(f.count("api_settings_audit").await, 1);
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(Uuid::new_v4())
        )
        .await
        .0,
        409
    );
    let (a, b) = tokio::join!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"2\""),
            settings(json!(5), 30.0, "progress")
        ),
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"2\""),
            settings(Value::Null, 40.0, "glass")
        )
    );
    let mut statuses = [a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 412]);
    assert_eq!(f.count("api_settings_audit").await, 2);
    assert_eq!(f.count("api_operations").await, 2);
    f.close().await;
}

#[tokio::test]
async fn failed_settings_audit_rolls_back_settings_and_receipt() {
    let f = Fixture::new().await;
    let initial = f
        .send(
            "GET",
            "/v1/me/settings",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client
        .batch_execute("ALTER TABLE api_settings_audit ADD CONSTRAINT reject_test CHECK (false)")
        .await
        .unwrap();
    let failed = f
        .send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            settings(Value::Null, 25.0, "glass"),
        )
        .await;
    assert_eq!(failed.0, 500);
    assert!(!failed.1.to_string().contains("reject_test"));
    assert_eq!(
        f.send(
            "GET",
            "/v1/me/settings",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await,
        initial
    );
    assert_eq!(f.count("api_settings_audit").await, 0);
    assert_eq!(f.count("api_operations").await, 0);
    f.close().await;
}

#[tokio::test]
async fn settings_defaults_persist_and_changes_are_private_versioned_and_replayable() {
    let mut f = Fixture::new().await;
    let initial = f
        .send(
            "GET",
            "/v1/me/settings",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(initial.0, 200);
    assert_eq!(
        initial.1,
        json!({"rounding_interval_minutes":5,"weekly_target_hours":35,"visual_indicator":"progress","version":1})
    );
    assert_eq!(initial.2.as_deref(), Some("\"1\""));
    let key = Uuid::new_v4();
    let body = settings(Value::Null, 32.5, "glass");
    let changed = f
        .send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(key),
            Some("\"1\""),
            body.clone(),
        )
        .await;
    assert_eq!(changed.0, 200);
    assert_eq!(changed.1["version"], 2);
    assert_eq!(changed.1["weekly_target_hours"], 32.5);
    assert_eq!(changed.1["visual_indicator"], "glass");
    assert!(changed.1["rounding_interval_minutes"].is_null());
    f.app = router(f.state.clone());
    assert_eq!(
        f.send(
            "GET",
            "/v1/me/settings",
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await,
        changed
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(key),
            Some("\"1\""),
            body.clone()
        )
        .await,
        changed
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            body
        )
        .await
        .0,
        412
    );
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(key),
            Some("\"1\""),
            settings(json!(5), 40.0, "progress")
        )
        .await
        .0,
        409
    );
    let other = f
        .send(
            "GET",
            "/v1/me/settings",
            Some(&f.other),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(other.1, initial.1);
    assert_eq!(f.count("api_settings_audit").await, 1);
    assert_eq!(f.count("api_operations").await, 1);
    f.close().await;
}

#[tokio::test]
async fn rounding_preserves_originals_ties_zero_and_old_entry_policy() {
    let f = Fixture::new().await;
    for (seconds, effective) in [
        (1_i64, 0_i64),
        (149, 0),
        (150, 300),
        (151, 300),
        (449, 300),
        (450, 600),
        (780, 900),
        (781, 900),
        (10800, 10800),
        (9_007_199_254_740_991, 9_007_199_254_740_900),
    ] {
        let mut body = entry(Uuid::new_v4());
        body["original_seconds"] = json!(seconds);
        let saved = f
            .send(
                "POST",
                "/v1/me/entries",
                Some(&f.token),
                Some(Uuid::new_v4()),
                None,
                body,
            )
            .await;
        assert_eq!(saved.0, 201);
        assert_eq!(saved.1["original_seconds"], seconds);
        assert_eq!(saved.1["effective_seconds"], effective);
        assert_eq!(saved.1["rounding_interval_minutes"], 5);
        assert_eq!(saved.1["export_state"], "not_exported");
    }
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let old = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id),
        )
        .await;
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            settings(Value::Null, 35.0, "progress")
        )
        .await
        .0,
        200
    );
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            entry(id)
        )
        .await,
        old
    );
    let fresh = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4()),
        )
        .await;
    assert_eq!(fresh.1["effective_seconds"], 781);
    assert!(fresh.1["rounding_interval_minutes"].is_null());
    let mut replacement = entry(id);
    replacement.as_object_mut().unwrap().remove("entry_id");
    replacement["original_seconds"] = json!(450);
    let edited = f
        .send(
            "PUT",
            &format!("/v1/me/entries/{id}"),
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            replacement,
        )
        .await;
    assert_eq!(edited.0, 200);
    assert_eq!(edited.1["effective_seconds"], 600);
    assert_eq!(edited.1["rounding_interval_minutes"], 5);
    let list = f
        .send(
            "GET",
            "/v1/me/entries?from_date=2026-10-26&to_date=2026-10-26",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert!(
        list.1["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["original_seconds"] == 1 && e["effective_seconds"] == 0)
    );
    f.close().await;
}
