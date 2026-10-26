use super::*;

#[tokio::test]
async fn project_cursors_expire_and_recheck_membership_on_each_page() {
    let f = Fixture::new().await;
    seed(&f, &f.token, false).await;
    let (tenant, person, _) = seed(&f, &f.token, false).await;
    for query in [
        "limit=0",
        "limit=101",
        "limit=-1",
        "cursor=bad",
        "person_id=other",
    ] {
        assert_eq!(
            f.send(
                "GET",
                &format!("/v1/projects?{query}"),
                Some(&f.token),
                None,
                None,
                Value::Null
            )
            .await
            .0,
            422
        );
    }
    let first = f
        .send(
            "GET",
            "/v1/projects?limit=1",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await
        .1;
    let cursor = first["next_cursor"].as_str().unwrap();
    let url = format!("/v1/projects?limit=1&cursor={cursor}");
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client
        .execute(
            "DELETE FROM api_project_members WHERE tenant_id=$1 AND person_id=$2",
            &[&tenant, &person],
        )
        .await
        .unwrap();
    assert_eq!(
        f.send("GET", &url, Some(&f.token), None, None, Value::Null)
            .await
            .1["items"],
        json!([])
    );
    client
        .batch_execute("UPDATE api_project_cursors SET expires_at=now()-interval '1 second'")
        .await
        .unwrap();
    assert_eq!(
        f.send("GET", &url, Some(&f.token), None, None, Value::Null)
            .await
            .0,
        422
    );
    f.close().await;
}

#[tokio::test]
async fn project_ids_and_cursors_do_not_cross_tenants_for_the_same_person() {
    let mut f = Fixture::new().await;
    let (_, person, project) = seed(&f, &f.token, false).await;
    seed(&f, &f.token, false).await;
    let first = f
        .send(
            "GET",
            "/v1/projects?limit=1",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await
        .1;
    let cursor = first["next_cursor"].as_str().unwrap();
    f.app = router(AppState::development(
        f.config.clone(),
        vec![(f.token.clone(), Uuid::new_v4(), person)],
    ));
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
            &format!("/v1/projects?limit=1&cursor={cursor}"),
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await
        .0,
        422
    );
    let mut body = entry(Uuid::new_v4());
    body["project_id"] = json!(project);
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
    assert_eq!(f.count("api_entries").await, 0);
    assert_eq!(f.count("api_operations").await, 0);
    f.close().await;
}

async fn seed(f: &Fixture, token: &str, archived: bool) -> (Uuid, Uuid, Uuid) {
    let identity = f
        .send("GET", "/v1/me", Some(token), None, None, Value::Null)
        .await
        .1;
    let tenant: Uuid = serde_json::from_value(identity["tenant_id"].clone()).unwrap();
    let person: Uuid = serde_json::from_value(identity["person_id"].clone()).unwrap();
    let project = Uuid::new_v4();
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client.execute("INSERT INTO api_projects(tenant_id,project_id,name,colour,archived) VALUES ($1,$2,'Private project','#ff00aa',$3)",&[&tenant,&project,&archived]).await.unwrap();
    client
        .execute(
            "INSERT INTO api_project_members(tenant_id,project_id,person_id) VALUES ($1,$2,$3)",
            &[&tenant, &project, &person],
        )
        .await
        .unwrap();
    (tenant, person, project)
}

#[tokio::test]
async fn project_list_is_private_and_cursors_are_bound_to_the_account() {
    let f = Fixture::new().await;
    assert_eq!(
        f.send("GET", "/v1/projects", None, None, None, Value::Null)
            .await
            .0,
        401
    );
    let empty = f
        .send(
            "GET",
            "/v1/projects",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(empty.0, 200);
    assert_eq!(empty.1, json!({"items":[],"next_cursor":null}));
    for archived in [false, false, true] {
        seed(&f, &f.token, archived).await;
    }
    let first = f
        .send(
            "GET",
            "/v1/projects?limit=2",
            Some(&f.token),
            None,
            None,
            Value::Null,
        )
        .await;
    assert_eq!(first.0, 200);
    assert_eq!(first.1["items"].as_array().unwrap().len(), 2);
    let cursor = first.1["next_cursor"].as_str().unwrap();
    let url = format!("/v1/projects?limit=2&cursor={cursor}");
    let second = f
        .send("GET", &url, Some(&f.token), None, None, Value::Null)
        .await;
    assert_eq!(second.0, 200);
    assert_eq!(second.1["items"].as_array().unwrap().len(), 1);
    assert!(second.1["next_cursor"].is_null());
    assert!(
        !first.1["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["project_id"] == second.1["items"][0]["project_id"])
    );
    assert_eq!(
        f.send("GET", &url, Some(&f.other), None, None, Value::Null)
            .await
            .0,
        422
    );
    assert_eq!(
        f.send(
            "GET",
            &format!("/v1/projects?limit=1&cursor={cursor}"),
            Some(&f.token),
            None,
            None,
            Value::Null
        )
        .await
        .0,
        422
    );
    assert_eq!(
        f.send(
            "GET",
            "/v1/projects",
            Some(&f.other),
            None,
            None,
            Value::Null
        )
        .await
        .1["items"],
        json!([])
    );
    f.close().await;
}

#[tokio::test]
async fn project_access_and_archiving_guard_entry_mutations() {
    let f = Fixture::new().await;
    let (tenant, person, project) = seed(&f, &f.token, false).await;
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let mut body = entry(id);
    body["project_id"] = json!(project);
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.other),
            Some(Uuid::new_v4()),
            None,
            body.clone()
        )
        .await
        .0,
        422
    );
    let accepted = f
        .send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(key),
            None,
            body.clone(),
        )
        .await;
    assert_eq!(accepted.0, 201);
    let mut timed = super::timed::timed_entry(Uuid::new_v4());
    timed["project_id"] = json!(project);
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            timed
        )
        .await
        .0,
        201
    );
    let (client, connection) = f.config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    client
        .execute(
            "UPDATE api_projects SET archived=true WHERE tenant_id=$1 AND project_id=$2",
            &[&tenant, &project],
        )
        .await
        .unwrap();
    let mut fresh = body.clone();
    fresh["entry_id"] = json!(Uuid::new_v4());
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.token),
            Some(Uuid::new_v4()),
            None,
            fresh
        )
        .await
        .0,
        422
    );
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
        accepted
    );
    body.as_object_mut().unwrap().remove("entry_id");
    body["description"] = json!("Corrected note");
    let path = format!("/v1/me/entries/{id}");
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
    body["original_seconds"] = json!(1000);
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
    client
        .execute(
            "DELETE FROM api_project_members WHERE tenant_id=$1 AND project_id=$2 AND person_id=$3",
            &[&tenant, &project, &person],
        )
        .await
        .unwrap();
    body["original_seconds"] = json!(781);
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
        422
    );
    assert_eq!(
        f.send("GET", &path, Some(&f.token), None, None, Value::Null)
            .await
            .0,
        200
    );
    assert_eq!(
        f.send(
            "DELETE",
            &path,
            Some(&f.token),
            Some(Uuid::new_v4()),
            Some("\"2\""),
            Value::Null
        )
        .await
        .0,
        204
    );
    assert_eq!(f.count("api_entries").await, 2);
    assert_eq!(f.count("api_audit").await, 4);
    f.close().await;
}
