use super::*;
use crate::transport::RemoteChange;

fn queue() -> Queue {
    Queue::prepare(
        Connection::open_in_memory().unwrap(),
        &Scope {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: Uuid::new_v4(),
            person_id: Uuid::new_v4(),
        },
    )
    .unwrap()
}
fn entry() -> Entry {
    Entry {
        recorded_settings: None,
        entry_id: Uuid::new_v4(),
        entry_date: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
        description: "Local work".into(),
        original_seconds: 900,
        project_id: None,
        timing: None,
    }
}

#[test]
fn edits_and_local_conflict_choices_cannot_change_recorded_settings() {
    let mut q = queue();
    let mut local = entry();
    local.recorded_settings = Some(RecordedSettings {
        rounding_interval_minutes: None,
    });
    let key = q.enqueue(&local).unwrap();
    q.confirm(key).unwrap();
    let mut changed = local.clone();
    changed
        .recorded_settings
        .as_mut()
        .unwrap()
        .rounding_interval_minutes = Some(5);
    assert!(q.replace(&changed).is_err());
    assert_eq!(q.entry(local.entry_id).unwrap(), Some(local.clone()));
    let key = q.replace(&local).unwrap();
    conflict(&mut q, &local, 2, Some(changed));
    assert!(q.resolve_with_local(key, 2).is_err());
    assert_eq!(
        q.resolve_as_new(key, 2).unwrap().recorded_settings,
        local.recorded_settings
    );
}

#[test]
fn recorded_settings_survive_queue_restart_and_copy_recovery() {
    let path = std::env::temp_dir().join(format!("recorded-{}.db", Uuid::new_v4()));
    let scope = queue().scope.clone();
    let mut q = Queue::open(&path, &scope).unwrap();
    let mut local = entry();
    local.recorded_settings = Some(RecordedSettings {
        rounding_interval_minutes: None,
    });
    let key = q.enqueue(&local).unwrap();
    drop(q);
    let mut q = Queue::open(&path, &scope).unwrap();
    assert_eq!(q.pending().unwrap()[0].operation_id, key);
    assert_eq!(
        q.pending().unwrap()[0].entry.recorded_settings,
        local.recorded_settings
    );
    conflict(&mut q, &local, 2, None);
    let copy = q.resolve_as_new(key, 2).unwrap();
    assert_eq!(copy.recorded_settings, local.recorded_settings);
    let mut invalid = entry();
    invalid.recorded_settings = Some(RecordedSettings {
        rounding_interval_minutes: Some(10),
    });
    assert!(q.enqueue(&invalid).is_err());
    drop(q);
    std::fs::remove_file(path).unwrap();
}
fn conflict(queue: &mut Queue, entry: &Entry, version: i64, remote: Option<Entry>) {
    queue
        .apply_remote_page(
            queue.checkpoint().unwrap(),
            Uuid::new_v4(),
            &[RemoteChange {
                entry_id: entry.entry_id,
                version,
                entry: remote,
            }],
        )
        .unwrap();
}

#[test]
fn accepting_remote_requires_the_reviewed_version_and_keeps_deletion_tombstones() {
    let mut q = queue();
    let local = entry();
    let operation = q.enqueue(&local).unwrap();
    let mut remote = local.clone();
    remote.description = "Remote work".into();
    conflict(&mut q, &local, 2, Some(remote.clone()));
    assert!(q.resolve_with_remote(operation, 1).is_err());
    assert_eq!(q.pending().unwrap()[0].operation_id, operation);
    q.resolve_with_remote(operation, 2).unwrap();
    assert_eq!(q.entry(local.entry_id).unwrap(), Some(remote));
    assert!(q.pending().unwrap().is_empty());
    let operation = q.replace(&local).unwrap();
    conflict(&mut q, &local, 3, None);
    q.resolve_with_remote(operation, 3).unwrap();
    assert!(q.entry(local.entry_id).unwrap().is_none());
    assert!(matches!(q.enqueue(&local), Err(Error::DuplicateEntry)));
}

#[test]
fn copying_local_work_uses_new_entry_and_operation_ids_and_preserves_remote() {
    for deleted in [false, true] {
        let mut q = queue();
        let local = entry();
        let operation = q.enqueue(&local).unwrap();
        let remote = (!deleted).then(|| {
            let mut e = local.clone();
            e.description = "Remote".into();
            e
        });
        conflict(&mut q, &local, 2, remote.clone());
        let created = q.resolve_as_new(operation, 2).unwrap();
        assert_ne!(created.entry_id, local.entry_id);
        assert_eq!(created.description, local.description);
        assert_eq!(q.entry(local.entry_id).unwrap(), remote);
        let pending = q.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_ne!(pending[0].operation_id, operation);
        assert_eq!(pending[0].entry, created);
    }
}

#[test]
fn keep_local_rejects_remote_deletion_and_stale_reviews_but_can_resolve_creation_collision() {
    let mut q = queue();
    let local = entry();
    let operation = q.enqueue(&local).unwrap();
    conflict(&mut q, &local, 2, Some(local.clone()));
    assert!(q.resolve_with_local(operation, 1).is_err());
    let replacement = q.resolve_with_local(operation, 2).unwrap();
    assert_ne!(replacement, operation);
    assert_eq!(
        q.operation_details(replacement).unwrap(),
        ("replace".into(), Some(2))
    );
    conflict(&mut q, &local, 3, None);
    assert!(q.resolve_with_local(replacement, 3).is_err());
    assert_eq!(q.pending().unwrap()[0].operation_id, replacement);
}

#[test]
fn failed_resolution_rolls_back_local_remote_and_operation_rows() {
    let mut q = queue();
    let local = entry();
    let operation = q.enqueue(&local).unwrap();
    conflict(&mut q, &local, 2, None);
    q.connection.execute_batch("CREATE TRIGGER fail_resolution BEFORE DELETE ON remote_conflicts BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(q.resolve_as_new(operation, 2).is_err());
    assert_eq!(q.entry(local.entry_id).unwrap(), Some(local.clone()));
    assert_eq!(q.pending().unwrap().len(), 1);
    assert_eq!(q.pending().unwrap()[0].operation_id, operation);
    assert!(q.remote_conflict(local.entry_id).unwrap().is_some());
}

#[test]
fn old_feed_pages_do_not_replace_a_newer_conflict_snapshot() {
    let mut q = queue();
    let local = entry();
    q.enqueue(&local).unwrap();
    conflict(&mut q, &local, 5, None);
    conflict(&mut q, &local, 3, Some(local.clone()));
    assert_eq!(
        q.remote_conflict(local.entry_id).unwrap().unwrap().version,
        5
    );
}
