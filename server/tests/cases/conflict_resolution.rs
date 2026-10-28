use super::*;

#[tokio::test]
async fn native_conflict_choices_keep_transfer_receipts_over_http() {
    use trackfast_sync::{
        Entry, Queue, Scope,
        native_resolution::Choice,
        transfer::{Change, Transfer},
        transport::{DevelopmentClient, SyncError},
    };
    let f = Fixture::new().await;
    let (tenant_id, person_id, _) = super::projects::seed(&f, &f.token, false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id,
        person_id,
    };
    let app = f.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let paths = [0, 1]
            .map(|_| std::env::temp_dir().join(format!("native-conflict-{}.db", Uuid::new_v4())));
        let mut local = Queue::open(&paths[0], &scope).unwrap();
        let mut remote = Queue::open(&paths[1], &scope).unwrap();
        let client = DevelopmentClient::new(scope.clone(), &token).unwrap();
        let mut entry = Entry {
            entry_id: Uuid::new_v4(),
            entry_date: "2026-10-28".parse().unwrap(),
            description: "Native original".into(),
            original_seconds: 780,
            recorded_settings: Some(trackfast_sync::RecordedSettings {
                rounding_interval_minutes: Some(5),
            }),
            project_id: None,
            timing: None,
        };
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: entry.entry_id,
            change: Change::Create(entry.clone()),
        };
        local.stage_transfer(&transfer).unwrap();
        client.sync(&mut local).unwrap();
        client.pull_changes(&mut remote).unwrap();
        entry.description = "Native correction".into();
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: entry.entry_id,
            change: Change::Replace(entry.clone()),
        };
        local.stage_transfer(&transfer).unwrap();
        remote
            .replace(&Entry {
                description: "Other client correction".into(),
                ..entry.clone()
            })
            .unwrap();
        client.sync(&mut remote).unwrap();
        assert!(matches!(client.sync(&mut local), Err(SyncError::Conflict)));
        let review = |queue: &mut Queue| {
            let mut cursor = None;
            loop {
                let page = client.read_changes(cursor).unwrap();
                queue.record_native_conflicts(&page.changes).unwrap();
                if !page.more {
                    break;
                }
                cursor = Some(page.cursor);
            }
        };
        review(&mut local);
        local
            .resolve_native(transfer.operation_id, 2, Choice::Local)
            .unwrap();
        drop(local);
        let mut local = Queue::open(&paths[0], &scope).unwrap();
        assert!(!local.stage_transfer(&transfer).unwrap());
        client.sync(&mut local).unwrap();
        assert!(local.stage_transfer(&transfer).unwrap());
        assert_eq!(
            client.read(entry.entry_id).unwrap().entry.description,
            "Native correction"
        );
        client.pull_changes(&mut remote).unwrap();
        remote.delete(entry.entry_id).unwrap();
        client.sync(&mut remote).unwrap();
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: entry.entry_id,
            change: Change::Replace(entry),
        };
        local.stage_transfer(&transfer).unwrap();
        assert!(matches!(client.sync(&mut local), Err(SyncError::Conflict)));
        review(&mut local);
        local
            .resolve_native(transfer.operation_id, 4, Choice::Copy)
            .unwrap();
        assert!(local.stage_transfer(&transfer).unwrap());
        assert!(
            local
                .native_resolution(transfer.operation_id)
                .unwrap()
                .unwrap()
                .remote
                .entry
                .is_none()
        );
        drop(local);
        drop(remote);
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
    })
    .await
    .unwrap();
    server.abort();
    f.close().await;
}

#[tokio::test]
async fn bounded_delivery_and_reviewed_conflicts_work_over_http() {
    use trackfast_sync::{
        Entry, Queue, Scope,
        transport::{DevelopmentClient, SyncError},
    };
    let f = Fixture::new().await;
    let (tenant_id, person_id, _) = super::projects::seed(&f, &f.token, false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scope = Scope {
        server: format!("http://{}", listener.local_addr().unwrap()),
        tenant_id,
        person_id,
    };
    let app = f.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let token = f.token.clone();
    tokio::task::spawn_blocking(move || {
        let paths =
            [0, 1].map(|_| std::env::temp_dir().join(format!("conflict-{}.db", Uuid::new_v4())));
        let mut remote = Queue::open(&paths[0], &scope).unwrap();
        let mut local = Queue::open(&paths[1], &scope).unwrap();
        let client = DevelopmentClient::new(scope, &token).unwrap();
        let mut entry = Entry {
            recorded_settings: Some(trackfast_sync::RecordedSettings {
                rounding_interval_minutes: None,
            }),
            entry_id: Uuid::new_v4(),
            entry_date: "2026-10-26".parse().unwrap(),
            description: "Original".into(),
            original_seconds: 900,
            project_id: None,
            timing: None,
        };
        remote.enqueue(&entry).unwrap();
        let mut second = entry.clone();
        second.entry_id = Uuid::new_v4();
        let second_key = remote.enqueue(&second).unwrap();
        assert!(client.sync_batch(&mut remote, 0).is_err());
        assert_eq!(client.sync_batch(&mut remote, 1).unwrap(), 1);
        assert_eq!(remote.pending().unwrap()[0].operation_id, second_key);
        assert_eq!(client.sync(&mut remote).unwrap(), 1);
        client.pull_changes(&mut local).unwrap();
        entry.description = "Local edit".into();
        let key = local.replace(&entry).unwrap();
        entry.description = "Remote edit".into();
        remote.replace(&entry).unwrap();
        client.sync(&mut remote).unwrap();
        assert!(matches!(client.sync(&mut local), Err(SyncError::Conflict)));
        client.pull_changes(&mut local).unwrap();
        local.resolve_with_remote(key, 2).unwrap();
        assert_eq!(local.entry(entry.entry_id).unwrap(), Some(entry.clone()));
        entry.description = "Preserved local work".into();
        let key = local.replace(&entry).unwrap();
        remote.delete(entry.entry_id).unwrap();
        client.sync(&mut remote).unwrap();
        assert!(matches!(client.sync(&mut local), Err(SyncError::Conflict)));
        client.pull_changes(&mut local).unwrap();
        let copy = local.resolve_as_new(key, 3).unwrap();
        assert_ne!(copy.entry_id, entry.entry_id);
        assert_eq!(copy.description, entry.description);
        assert_eq!(client.sync(&mut local).unwrap(), 1);
        assert_eq!(client.read(copy.entry_id).unwrap().entry, copy);
        assert!(local.entry(entry.entry_id).unwrap().is_none());
        drop(local);
        drop(remote);
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
    })
    .await
    .unwrap();
    server.abort();
    f.close().await;
}
