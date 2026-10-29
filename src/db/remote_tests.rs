use super::*;
use trackfast_sync::{
    Entry, RecordedSettings, Scope,
    native_download::{Incoming, RemoteOutcome},
    transport::RemoteChange,
};
use uuid::Uuid;

fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}
fn incoming() -> Incoming {
    let id = Uuid::new_v4();
    Incoming {
        id: Uuid::new_v4(),
        change: RemoteChange {
            applied_settings: None,
            entry_id: id,
            version: 1,
            entry: Some(Entry {
                entry_id: id,
                entry_date: "2026-10-29".parse().unwrap(),
                description: "Remote entry".into(),
                original_seconds: 780,
                project_id: None,
                timing: None,
                recorded_settings: Some(RecordedSettings {
                    rounding_interval_minutes: Some(5),
                }),
            }),
        },
    }
}
#[test]
fn remote_create_edit_delete_and_replay_preserve_identity_and_totals() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    assert_eq!(
        store.apply_remote(&scope, &incoming).unwrap(),
        RemoteOutcome::Applied
    );
    assert_eq!(
        store.apply_remote(&scope, &incoming).unwrap(),
        RemoteOutcome::Applied
    );
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    let id = snapshot.sessions[0].id;
    assert_eq!(
        snapshot.sessions[0]
            .elapsed(Timestamp::UNIX_EPOCH)
            .as_secs(),
        780
    );
    assert_eq!(
        snapshot.sessions[0]
            .effective(Timestamp::UNIX_EPOCH)
            .as_secs(),
        900
    );
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
    incoming.id = Uuid::new_v4();
    incoming.change.version = 2;
    incoming.change.entry.as_mut().unwrap().description = "API edit".into();
    store.apply_remote(&scope, &incoming).unwrap();
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    assert_eq!(snapshot.sessions[0].id, id);
    assert_eq!(snapshot.sessions[0].description, "API edit");
    incoming.id = Uuid::new_v4();
    incoming.change.version = 3;
    incoming.change.entry = None;
    store.apply_remote(&scope, &incoming).unwrap();
    assert!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sessions
            .is_empty()
    );
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
}
#[test]
fn a_local_edit_during_download_enters_conflict_review_and_survives_restart() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    store.apply_remote(&scope, &incoming).unwrap();
    let mut local = store
        .snapshot(Timestamp::UNIX_EPOCH)
        .unwrap()
        .sessions
        .remove(0);
    local.description = "Local edit".into();
    store.save_entry(&local).unwrap();
    incoming.id = Uuid::new_v4();
    incoming.change.version = 2;
    incoming.change.entry.as_mut().unwrap().description = "API edit".into();
    let outcome = store.apply_remote(&scope, &incoming).unwrap();
    assert!(matches!(outcome, RemoteOutcome::Conflict(_)));
    let mut store = Store::prepare(store.connection).unwrap();
    assert_eq!(store.apply_remote(&scope, &incoming).unwrap(), outcome);
    assert_eq!(
        store.snapshot(Timestamp::UNIX_EPOCH).unwrap().sessions[0].description,
        "Local edit"
    );
}

#[test]
fn legacy_rounding_is_displayed_but_does_not_change_the_api_policy() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    incoming.change.entry.as_mut().unwrap().recorded_settings = None;
    incoming.change.applied_settings = Some(RecordedSettings {
        rounding_interval_minutes: Some(5),
    });
    store.apply_remote(&scope, &incoming).unwrap();
    let mut session = store
        .snapshot(Timestamp::UNIX_EPOCH)
        .unwrap()
        .sessions
        .remove(0);
    assert_eq!(session.effective(Timestamp::UNIX_EPOCH).as_secs(), 900);
    session.description = "Edited legacy entry".into();
    store.save_entry(&session).unwrap();
    let transfer = store.prepare_delivery(&scope).unwrap().unwrap();
    let trackfast_sync::transfer::Change::Replace(entry) = transfer.change else {
        panic!("replacement required")
    };
    assert_eq!(entry.recorded_settings, None);
}

#[test]
fn missing_project_mapping_rolls_back_and_retry_uses_the_same_download() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    let project = Uuid::new_v4();
    incoming.change.entry.as_mut().unwrap().project_id = Some(project);
    let error = store
        .apply_remote(&scope, &incoming)
        .unwrap_err()
        .to_string();
    assert!(error.contains(&project.to_string()));
    assert!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sessions
            .is_empty()
    );
    let local = store
        .save_project(&ProjectDraft {
            id: None,
            name: "Remote project".into(),
            colour: "#aabbcc".into(),
            client: String::new(),
        })
        .unwrap();
    store.map_project(&scope, local, project).unwrap();
    store.apply_remote(&scope, &incoming).unwrap();
    assert_eq!(
        store.snapshot(Timestamp::UNIX_EPOCH).unwrap().sessions[0].project_id,
        Some(local)
    );
}

#[test]
fn replay_after_a_local_edit_preserves_the_original_acknowledgement() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let incoming = incoming();
    store.apply_remote(&scope, &incoming).unwrap();
    let mut local = store
        .snapshot(Timestamp::UNIX_EPOCH)
        .unwrap()
        .sessions
        .remove(0);
    local.description = "Later local edit".into();
    store.save_entry(&local).unwrap();
    assert_eq!(
        store.apply_remote(&scope, &incoming).unwrap(),
        RemoteOutcome::Applied
    );
    assert_eq!(
        store.snapshot(Timestamp::UNIX_EPOCH).unwrap().sessions[0].description,
        local.description
    );
    assert!(store.prepare_delivery(&scope).unwrap().is_some());
    let mut changed = incoming.clone();
    changed.change.version += 1;
    assert!(store.apply_remote(&scope, &changed).is_err());
    assert!(
        store
            .apply_remote(&super::remote_tests::scope(), &incoming)
            .is_err()
    );
}

#[test]
fn remote_deletion_conflicts_with_local_edits_and_local_deletion_conflicts_with_remote_edits() {
    for local_delete in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let scope = scope();
        store.enroll(&scope).unwrap();
        let mut incoming = incoming();
        store.apply_remote(&scope, &incoming).unwrap();
        let mut local = store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sessions
            .remove(0);
        if local_delete {
            store.delete_entry(local.id).unwrap();
        } else {
            local.description = "Local edit".into();
            store.save_entry(&local).unwrap();
        }
        incoming.id = Uuid::new_v4();
        incoming.change.version = 2;
        if !local_delete {
            incoming.change.entry = None;
        }
        let RemoteOutcome::Conflict(transfer) = store.apply_remote(&scope, &incoming).unwrap()
        else {
            panic!("conflict required")
        };
        assert_eq!(store.inspect_delivery(&scope).unwrap(), Some(transfer));
        assert_eq!(
            store
                .snapshot(Timestamp::UNIX_EPOCH)
                .unwrap()
                .sessions
                .is_empty(),
            local_delete
        );
    }
}

#[test]
fn remote_restore_keeps_identity_and_old_versions_do_not_replace_new_work() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    let original = incoming.change.entry.take();
    store.apply_remote(&scope, &incoming).unwrap();
    incoming.id = Uuid::new_v4();
    incoming.change.version = 2;
    incoming.change.entry = original;
    store.apply_remote(&scope, &incoming).unwrap();
    let mut stale = incoming.clone();
    stale.id = Uuid::new_v4();
    stale.change.version = 1;
    stale.change.entry = None;
    store.apply_remote(&scope, &stale).unwrap();
    assert_eq!(
        store
            .snapshot(Timestamp::UNIX_EPOCH)
            .unwrap()
            .sessions
            .len(),
        1
    );
    assert!(store.prepare_delivery(&scope).unwrap().is_none());
}

#[test]
fn copying_deleted_local_work_keeps_the_legacy_rounding_rule() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    let mut incoming = incoming();
    incoming.change.entry.as_mut().unwrap().recorded_settings = None;
    incoming.change.applied_settings = Some(RecordedSettings {
        rounding_interval_minutes: Some(5),
    });
    store.apply_remote(&scope, &incoming).unwrap();
    let mut local = store
        .snapshot(Timestamp::UNIX_EPOCH)
        .unwrap()
        .sessions
        .remove(0);
    local.description = "Local change".into();
    store.save_entry(&local).unwrap();
    incoming.id = Uuid::new_v4();
    incoming.change.version = 2;
    incoming.change.entry = None;
    incoming.change.applied_settings = None;
    let RemoteOutcome::Conflict(transfer) = store.apply_remote(&scope, &incoming).unwrap() else {
        panic!("conflict required")
    };
    store.delete_entry(local.id).unwrap();
    store
        .apply_resolution(
            &scope,
            transfer.operation_id,
            &trackfast_sync::native_resolution::Resolution {
                choice: trackfast_sync::native_resolution::Choice::Copy,
                remote: incoming.change,
            },
        )
        .unwrap();
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(
        snapshot.sessions[0]
            .effective(Timestamp::UNIX_EPOCH)
            .as_secs(),
        900
    );
}
