use super::*;
use trackfast_sync::{Scope, transfer::Change};
use uuid::Uuid;
fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}
fn entry() -> Session {
    Session {
        id: 0,
        description: "Work".into(),
        project_id: None,
        start: Timestamp::UNIX_EPOCH,
        stop: Some(Timestamp::UNIX_EPOCH),
        details: crate::entries::EntryDetails {
            duration_seconds: Some(781),
            entry_date: Some("2026-10-28".parse().unwrap()),
            ..Default::default()
        },
    }
}

#[test]
fn delivery_freezes_content_and_preserves_edits_until_receipt() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.save_entry(&entry()).unwrap();
    store.enroll(&scope).unwrap();
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
    store.save_entry(&entry()).unwrap();
    let first = store.prepare_delivery(&scope).unwrap().unwrap();
    let Change::Create(created) = &first.change else {
        panic!("expected create");
    };
    assert_eq!(created.original_seconds, 781);
    assert_eq!(
        created
            .recorded_settings
            .as_ref()
            .unwrap()
            .rounding_interval_minutes,
        Some(5)
    );
    let mut edit = entry();
    edit.id = 2;
    edit.description = "Edited while offline".into();
    store.save_entry(&edit).unwrap();
    let mut store = Store::prepare(store.connection).unwrap();
    assert_eq!(store.prepare_delivery(&scope).unwrap().unwrap(), first);
    assert!(store.confirm_delivery(&scope, Uuid::new_v4()).is_err());
    store.confirm_delivery(&scope, first.operation_id).unwrap();
    let second = store.prepare_delivery(&scope).unwrap().unwrap();
    assert!(matches!(&second.change,Change::Replace(e) if e.description==edit.description));
    assert_ne!(first.operation_id, second.operation_id);
    store.delete_entry(2).unwrap();
    assert_eq!(store.prepare_delivery(&scope).unwrap().unwrap(), second);
    store.confirm_delivery(&scope, second.operation_id).unwrap();
    let deletion = store.prepare_delivery(&scope).unwrap().unwrap();
    assert_eq!(deletion.change, Change::Delete);
    store
        .confirm_delivery(&scope, deletion.operation_id)
        .unwrap();
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
}

#[test]
fn unmapped_work_waits_and_deleted_unsent_work_does_not_upload() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let local = store
        .save_project(&ProjectDraft {
            name: "Project".into(),
            colour: "#abcdef".into(),
            ..Default::default()
        })
        .unwrap();
    let mut draft = entry();
    draft.project_id = Some(local);
    store.save_entry(&draft).unwrap();
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
    let remote = Uuid::new_v4();
    store.map_project(&scope, local, remote).unwrap();
    let work = store.prepare_delivery(&scope).unwrap().unwrap();
    assert!(matches!(work.change,Change::Create(ref e) if e.project_id==Some(remote)));
    assert!(
        store
            .prepare_delivery(&Scope {
                person_id: Uuid::new_v4(),
                ..scope.clone()
            })
            .is_err()
    );
    store.confirm_delivery(&scope, work.operation_id).unwrap();
    store.save_entry(&entry()).unwrap();
    store.delete_entry(2).unwrap();
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
}

#[test]
fn native_and_queue_reopen_recover_a_receipt_not_yet_saved_locally() {
    let root = std::env::temp_dir();
    let id = Uuid::new_v4();
    let native = root.join(format!("trackfast-native-{id}.db"));
    let queued = root.join(format!("trackfast-queue-{id}.db"));
    let scope = scope();
    let transfer = {
        let mut store = Store::open(&native).unwrap();
        store.enroll(&scope).unwrap();
        store.save_entry(&entry()).unwrap();
        let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
        let mut queue = trackfast_sync::Queue::open(&queued, &scope).unwrap();
        assert!(!queue.stage_transfer(&transfer).unwrap());
        queue.confirm(transfer.operation_id).unwrap();
        transfer
    };
    {
        let mut store = Store::open(&native).unwrap();
        let mut queue = trackfast_sync::Queue::open(&queued, &scope).unwrap();
        assert_eq!(store.prepare_delivery(&scope).unwrap().unwrap(), transfer);
        assert!(queue.stage_transfer(&transfer).unwrap());
        store
            .confirm_delivery(&scope, transfer.operation_id)
            .unwrap();
        assert!(store.prepare_delivery(&scope).unwrap().is_none());
        assert!(queue.pending().unwrap().is_empty());
    }
    std::fs::remove_file(native).unwrap();
    std::fs::remove_file(queued).unwrap();
}

#[test]
fn timed_delivery_preserves_endpoints_and_rounding_off() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    store
        .save_settings(&crate::entries::Settings {
            round_to_five: false,
            ..Default::default()
        })
        .unwrap();
    let start: Timestamp = "2026-10-28T09:03:00Z".parse().unwrap();
    store
        .add_task(
            &Task {
                description: "Timed work".into(),
                project_id: None,
            },
            start,
        )
        .unwrap();
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
    store
        .pause(1, "2026-10-28T09:16:00Z".parse().unwrap())
        .unwrap();
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    let Change::Create(entry) = transfer.change else {
        panic!("expected create");
    };
    assert_eq!(entry.original_seconds, 780);
    assert_eq!(
        entry.recorded_settings.unwrap().rounding_interval_minutes,
        None
    );
    let timing = entry.timing.unwrap();
    assert_eq!(timing.started_at.parse::<Timestamp>().unwrap(), start);
    assert_eq!(
        timing.stopped_at.parse::<Timestamp>().unwrap(),
        "2026-10-28T09:16:00Z".parse::<Timestamp>().unwrap()
    );
}

#[test]
fn failed_native_receipt_keeps_the_outbox_and_unconfirmed_revision() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    store.save_entry(&entry()).unwrap();
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    store.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE DELETE ON sync_outbox BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        store
            .confirm_delivery(&scope, transfer.operation_id)
            .is_err()
    );
    let count: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM sync_delivered", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(store.prepare_delivery(&scope).unwrap().unwrap(), transfer);
}

#[test]
fn entry_sync_status_tracks_receipts_and_preserves_attention_across_restart() {
    use crate::entries::SyncStatus;
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.save_entry(&entry()).unwrap();
    store.enroll(&scope).unwrap();
    store.save_entry(&entry()).unwrap();
    let status = |store: &Store, id| {
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(id)
    };
    assert_eq!(status(&store, 1), SyncStatus::LocalOnly);
    assert_eq!(status(&store, 2), SyncStatus::Waiting);
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    assert!(
        store
            .mark_delivery_attention(&scope, Uuid::new_v4())
            .is_err()
    );
    store
        .mark_delivery_attention(&scope, transfer.operation_id)
        .unwrap();
    let mut store = Store::prepare(store.connection).unwrap();
    assert_eq!(status(&store, 2), SyncStatus::NeedsAttention);
    store
        .confirm_delivery(&scope, transfer.operation_id)
        .unwrap();
    assert_eq!(status(&store, 2), SyncStatus::Confirmed);
    let mut changed = entry();
    changed.id = 2;
    changed.description = "Changed".into();
    store.save_entry(&changed).unwrap();
    assert_eq!(status(&store, 2), SyncStatus::Waiting);
}

#[test]
fn sync_status_explains_mapping_running_and_imported_work() {
    use crate::entries::SyncStatus;
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let project = store
        .save_project(&ProjectDraft {
            name: "Local project".into(),
            colour: "#abcdef".into(),
            ..Default::default()
        })
        .unwrap();
    let mut draft = entry();
    draft.project_id = Some(project);
    store.save_entry(&draft).unwrap();
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(1),
        SyncStatus::NeedsMapping
    );
    store.map_project(&scope, project, Uuid::new_v4()).unwrap();
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(1),
        SyncStatus::Waiting
    );
    store
        .add_task(
            &Task {
                description: "Running".into(),
                project_id: None,
            },
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(2),
        SyncStatus::WaitingForStop
    );
    store.save_entry(&entry()).unwrap();
    store
        .connection
        .execute("UPDATE sessions SET toggl_id=123 WHERE id=3", [])
        .unwrap();
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(3),
        SyncStatus::LocalOnly
    );
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    let other = Scope {
        person_id: Uuid::new_v4(),
        ..scope.clone()
    };
    assert!(
        store
            .mark_delivery_attention(&other, transfer.operation_id)
            .is_err()
    );
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sync_status(1),
        SyncStatus::Waiting
    );
}

#[test]
fn remote_resolution_preserves_later_edits_and_replays_without_duplicates() {
    use trackfast_sync::{
        native_resolution::{Choice, Resolution},
        transport::RemoteChange,
    };
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    store.save_entry(&entry()).unwrap();
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    let Change::Create(mut api) = transfer.change.clone() else {
        panic!()
    };
    api.description = "API version".into();
    let mut local = entry();
    local.id = 1;
    local.description = "Later local edit".into();
    store.save_entry(&local).unwrap();
    let resolution = Resolution {
        choice: Choice::Remote,
        remote: RemoteChange {
            entry_id: transfer.entry_id,
            version: 2,
            entry: Some(api),
        },
    };
    store
        .apply_resolution(&scope, transfer.operation_id, &resolution)
        .unwrap();
    store
        .apply_resolution(&scope, transfer.operation_id, &resolution)
        .unwrap();
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    assert_eq!(snapshot.sessions.len(), 2);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == 1)
            .unwrap()
            .description,
        "API version"
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|s| s.description == "Later local edit")
    );
    assert_eq!(
        snapshot.sync_status(1),
        crate::entries::SyncStatus::Confirmed
    );
    let next = store.prepare_delivery(&scope).unwrap().unwrap();
    assert!(matches!(next.change, Change::Create(ref e) if e.description=="Later local edit"));
    assert_ne!(next.entry_id, transfer.entry_id);
}

#[test]
fn accepted_remote_deletion_and_copy_retain_native_identity_rules() {
    use trackfast_sync::{
        native_resolution::{Choice, Resolution},
        transport::RemoteChange,
    };
    for choice in [Choice::Remote, Choice::Copy] {
        let mut store = Store::open_in_memory().unwrap();
        let scope = scope();
        store.enroll(&scope).unwrap();
        store.save_entry(&entry()).unwrap();
        let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
        let resolution = Resolution {
            choice,
            remote: RemoteChange {
                entry_id: transfer.entry_id,
                version: 2,
                entry: None,
            },
        };
        store
            .apply_resolution(&scope, transfer.operation_id, &resolution)
            .unwrap();
        store
            .apply_resolution(&scope, transfer.operation_id, &resolution)
            .unwrap();
        let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
        assert_eq!(snapshot.sessions.len(), usize::from(choice == Choice::Copy));
        let pending = store.prepare_delivery(&scope).unwrap();
        assert_eq!(pending.is_some(), choice == Choice::Copy);
        if let Some(pending) = pending {
            assert_ne!(pending.entry_id, transfer.entry_id);
        }
    }
}

#[test]
fn conflict_application_rolls_back_and_preserves_later_deletions() {
    use trackfast_sync::{
        native_resolution::{Choice, Resolution},
        transport::RemoteChange,
    };
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    store.save_entry(&entry()).unwrap();
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    let Change::Create(api) = transfer.change.clone() else {
        panic!()
    };
    let resolution = Resolution {
        choice: Choice::Remote,
        remote: RemoteChange {
            entry_id: transfer.entry_id,
            version: 2,
            entry: Some(api),
        },
    };
    store.delete_entry(1).unwrap();
    store.connection.execute_batch("CREATE TRIGGER fail_apply BEFORE INSERT ON sync_applied_resolutions BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        store
            .apply_resolution(&scope, transfer.operation_id, &resolution)
            .is_err()
    );
    assert_eq!(store.prepare_delivery(&scope).unwrap().unwrap(), transfer);
    store
        .connection
        .execute_batch("DROP TRIGGER fail_apply;")
        .unwrap();
    store
        .apply_resolution(&scope, transfer.operation_id, &resolution)
        .unwrap();
    assert!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sessions
            .is_empty()
    );
    assert_eq!(
        store.prepare_delivery(&scope).unwrap().unwrap().change,
        Change::Delete
    );
}

#[test]
fn conflict_choice_survives_both_files_reopening_before_native_application() {
    use trackfast_sync::{native_resolution::Choice, transport::RemoteChange};
    let scope = scope();
    let paths = [0, 1]
        .map(|_| std::env::temp_dir().join(format!("native-resolution-{}.db", Uuid::new_v4())));
    let transfer = {
        let mut store = Store::open(&paths[0]).unwrap();
        store.enroll(&scope).unwrap();
        store.save_entry(&entry()).unwrap();
        let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
        let mut queue = trackfast_sync::Queue::open(&paths[1], &scope).unwrap();
        queue.stage_transfer(&transfer).unwrap();
        drop(queue);
        {
            let connection = rusqlite::Connection::open(&paths[1]).unwrap();
            connection
                .execute(
                    "UPDATE pending SET conflict=1 WHERE operation_id=?1",
                    [transfer.operation_id.to_string()],
                )
                .unwrap();
        }
        let mut queue = trackfast_sync::Queue::open(&paths[1], &scope).unwrap();
        let Change::Create(mut remote) = transfer.change.clone() else {
            panic!()
        };
        remote.description = "Accepted remote".into();
        queue
            .record_native_conflicts(&[RemoteChange {
                entry_id: transfer.entry_id,
                version: 1,
                entry: Some(remote),
            }])
            .unwrap();
        queue
            .resolve_native(transfer.operation_id, 1, Choice::Copy)
            .unwrap();
        transfer
    };
    {
        let mut store = Store::open(&paths[0]).unwrap();
        let mut queue = trackfast_sync::Queue::open(&paths[1], &scope).unwrap();
        assert!(queue.stage_transfer(&transfer).unwrap());
        let resolution = queue
            .native_resolution(transfer.operation_id)
            .unwrap()
            .unwrap();
        store
            .apply_resolution(&scope, transfer.operation_id, &resolution)
            .unwrap();
    }
    {
        let mut store = Store::open(&paths[0]).unwrap();
        let queue = trackfast_sync::Queue::open(&paths[1], &scope).unwrap();
        store
            .apply_resolution(
                &scope,
                transfer.operation_id,
                &queue
                    .native_resolution(transfer.operation_id)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        let data = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
        assert_eq!(data.sessions.len(), 2);
        assert!(
            data.sessions
                .iter()
                .any(|s| s.description == "Accepted remote")
        );
        assert!(data.sessions.iter().all(|s| s.details.round_to_five));
    }
    for path in paths {
        std::fs::remove_file(path).unwrap();
    }
}
