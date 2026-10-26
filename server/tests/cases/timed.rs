use super::*;

pub(super) fn timed_entry(id: Uuid) -> Value {
    json!({"entry_id":id,"kind":"timed","entry_date":"2026-10-26","project_id":null,"description":"Timed work","started_at":"2026-10-26T09:03:00+02:00","stopped_at":"2026-10-26T09:16:00+02:00","time_zone":"Africa/Johannesburg"})
}

#[tokio::test]
async fn timed_entries_derive_seconds_and_preserve_dates_and_retry_results() {
    let f = Fixture::new().await;
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let body = timed_entry(id);
    let created = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            body.clone(),
        )
        .await;
    assert_eq!(created.0, 201);
    assert_eq!(created.1["original_seconds"], 780);
    assert_eq!(created.1["effective_seconds"], 900);
    assert_eq!(created.1["time_zone"], "Africa/Johannesburg");
    let off = json!({"rounding_interval_minutes":null,"weekly_target_hours":35,"visual_indicator":"progress"});
    assert_eq!(
        f.send(
            "PUT",
            "/v1/me/settings",
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            off
        )
        .await
        .0,
        200
    );
    let exact = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            timed_entry(Uuid::new_v4()),
        )
        .await;
    assert_eq!(exact.1["effective_seconds"], 780);
    assert!(exact.1["rounding_interval_minutes"].is_null());
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            body.clone()
        )
        .await,
        created
    );
    let mut update = body;
    update.as_object_mut().unwrap().remove("entry_id");
    update["stopped_at"] = json!("2026-10-26T10:03:00+02:00");
    let replaced = f
        .send(
            "PUT",
            &format!("/v1/me/entries/{id}"),
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"1\""),
            update,
        )
        .await;
    assert_eq!(replaced.0, 200);
    assert_eq!(replaced.1["original_seconds"], 3600);
    assert_eq!(replaced.1["rounding_interval_minutes"], 5);
    assert_eq!(replaced.1["entry_date"], "2026-10-26");
    assert_eq!(
        f.send(
            "GET",
            &format!("/v1/me/entries/{id}"),
            Some(&f.other),
            None,
            None,
            Value::Null
        )
        .await
        .0,
        404
    );
    f.close().await;
}

#[tokio::test]
async fn timed_entries_use_elapsed_seconds_across_midnight_and_dst() {
    let f = Fixture::new().await;
    for (zone, date, start, stop, seconds) in [
        (
            "Africa/Johannesburg",
            "2026-10-26",
            "2026-10-26T23:50:00+02:00",
            "2026-10-27T00:10:00+02:00",
            1200,
        ),
        (
            "America/New_York",
            "2026-11-01",
            "2026-11-01T01:30:00-04:00",
            "2026-11-01T01:30:00-05:00",
            3600,
        ),
        (
            "America/New_York",
            "2026-03-08",
            "2026-03-08T01:30:00-05:00",
            "2026-03-08T03:30:00-04:00",
            3600,
        ),
    ] {
        let mut body = timed_entry(Uuid::new_v4());
        body["time_zone"] = json!(zone);
        body["entry_date"] = json!(date);
        body["started_at"] = json!(start);
        body["stopped_at"] = json!(stop);
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
        assert_eq!(saved.1["entry_date"], date);
    }
    assert_eq!(f.count("api_entries").await, 3);
    f.close().await;
}

#[tokio::test]
async fn invalid_timed_entries_do_not_write() {
    let f = Fixture::new().await;
    for (field, value) in [
        ("stopped_at", json!("2026-10-26T09:03:00+02:00")),
        ("stopped_at", json!("2026-10-26T09:00:00+02:00")),
        ("started_at", json!("2026-10-26T09:03:00")),
        ("started_at", json!("2026-10-26T09:03:00.5+02:00")),
        ("started_at", json!("2026-10-26T09:03:00+03:00")),
        ("entry_date", json!("2026-10-25")),
        ("time_zone", json!("Unknown/Zone")),
        ("original_seconds", json!(780)),
    ] {
        let mut body = timed_entry(Uuid::new_v4());
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
            422,
            "{field}"
        );
    }
    assert_eq!(f.count("api_entries").await, 0);
    assert_eq!(f.count("api_operations").await, 0);
    f.close().await;
}
