use super::*;

#[tokio::test]
async fn recorded_rounding_survives_settings_changes_replays_and_edits() {
    let f = Fixture::new().await;
    for (timed, interval) in [
        (false, Value::Null),
        (false, json!(5)),
        (true, Value::Null),
        (true, json!(5)),
    ] {
        let id = Uuid::new_v4();
        let key = Uuid::new_v4();
        let mut body = entry(id);
        if timed {
            body.as_object_mut().unwrap().remove("original_seconds");
            body["kind"] = json!("timed");
            body["started_at"] = json!("2026-10-26T09:03:00+02:00");
            body["stopped_at"] = json!("2026-10-26T09:16:01+02:00");
            body["time_zone"] = json!("Africa/Johannesburg");
        }
        body["recorded_settings"] = json!({"rounding_interval_minutes":interval});
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
        assert_eq!(created.1["rounding_interval_minutes"], interval);
        assert_eq!(
            created.1["effective_seconds"],
            if interval.is_null() { 781 } else { 900 }
        );
        let settings = f
            .send(
                "GET",
                "/v1/me/settings",
                Some(&f.token),
                None,
                None,
                Value::Null,
            )
            .await
            .1;
        let etag = format!("\"{}\"", settings["version"].as_i64().unwrap());
        assert_eq!(f.send("PUT", "/v1/me/settings", Some(&f.token), Some(Uuid::new_v4()), Some(&etag),
            json!({"rounding_interval_minutes":if interval.is_null() {json!(5)} else {Value::Null}, "weekly_target_hours":35,"visual_indicator":"progress"})).await.0, 200);
        assert_eq!(
            f.send(
                "POST",
                "/v1/me/entries",
                Some(&f.token),
                Some(key),
                None,
                body.clone()
            )
            .await
            .1,
            created.1
        );
        body["recorded_settings"]["rounding_interval_minutes"] = if interval.is_null() {
            json!(5)
        } else {
            Value::Null
        };
        assert_eq!(
            f.send(
                "POST",
                "/v1/me/entries",
                Some(&f.token),
                Some(key),
                None,
                body.clone()
            )
            .await
            .0,
            409
        );
        body.as_object_mut().unwrap().remove("entry_id");
        let url = format!("/v1/me/entries/{id}");
        assert_eq!(
            f.send(
                "PUT",
                &url,
                Some(&f.token),
                Some(Uuid::new_v4()),
                Some("\"1\""),
                body.clone()
            )
            .await
            .0,
            422
        );
        body.as_object_mut().unwrap().remove("recorded_settings");
        body["description"] = json!("Corrected description");
        let edited = f
            .send(
                "PUT",
                &url,
                Some(&f.token),
                Some(Uuid::new_v4()),
                Some("\"1\""),
                body,
            )
            .await;
        assert_eq!(edited.0, 200);
        assert_eq!(edited.1["rounding_interval_minutes"], interval);
        assert_eq!(
            edited.1["recorded_settings"],
            created.1["recorded_settings"]
        );
    }
    for snapshot in [
        Value::Null,
        json!({}),
        json!({"rounding_interval_minutes":10}),
        json!({"rounding_interval_minutes":5,"effective_seconds":900}),
    ] {
        let mut body = entry(Uuid::new_v4());
        body["recorded_settings"] = snapshot;
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
            422
        );
    }
    assert_eq!(f.count("api_entries").await, 4);
    assert_eq!(f.count("api_audit").await, 8);
    f.close().await;
}
