use super::*;

#[tokio::test]
async fn native_download_is_read_only_and_retains_rounding_and_restart_progress() {
    use trackfast_sync::{
        Queue, Scope, native_download::RemoteOutcome, transport::DevelopmentClient,
    };
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
        201
    );
    assert_eq!(
        f.send(
            "POST",
            "/v1/me/entries",
            Some(&f.other),
            Some(Uuid::new_v4()),
            None,
            entry(Uuid::new_v4())
        )
        .await
        .0,
        201
    );
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
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let path =
            std::env::temp_dir().join(format!("trackfast-native-download-{}.db", Uuid::new_v4()));
        let client = DevelopmentClient::new(scope.clone(), &token).unwrap();
        let mut queue = Queue::open(&path, &scope).unwrap();
        assert!(client.download_native_page(&mut queue).unwrap());
        let incoming = queue.next_native_remote().unwrap().unwrap();
        assert_eq!(incoming.change.entry_id, id);
        assert_eq!(
            incoming.change.entry.as_ref().unwrap().recorded_settings,
            None
        );
        assert_eq!(
            incoming
                .change
                .applied_settings
                .as_ref()
                .unwrap()
                .rounding_interval_minutes,
            Some(5)
        );
        assert_eq!(queue.native_download_cursor().unwrap(), None);
        assert!(queue.entry(id).unwrap().is_none());
        drop(queue);
        let mut queue = Queue::open(&path, &scope).unwrap();
        assert!(client.download_native_page(&mut queue).unwrap());
        assert_eq!(queue.next_native_remote().unwrap(), Some(incoming.clone()));
        queue
            .acknowledge_native_remote(&incoming, &RemoteOutcome::Applied)
            .unwrap();
        assert!(queue.native_download_cursor().unwrap().is_some());
        assert!(!client.download_native_page(&mut queue).unwrap());
        assert!(queue.next_native_remote().unwrap().is_none());
        assert!(queue.pending().unwrap().is_empty());
        drop(queue);
        std::fs::remove_file(path).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(f.count("api_entries").await, 2);
    assert_eq!(f.count("api_operations").await, 2);
    assert_eq!(f.count("api_audit").await, 2);
    server.abort();
    f.close().await;
}
