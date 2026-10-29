use crate::native_resolution::Choice;
use crate::transfer::{Change, Transfer};
use crate::transport::RemoteChange;
use crate::*;

fn fixture() -> (Queue, Transfer, RemoteChange) {
    let scope = Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    };
    let mut queue =
        Queue::prepare(rusqlite::Connection::open_in_memory().unwrap(), &scope).unwrap();
    let entry = Entry {
        entry_id: Uuid::new_v4(),
        project_id: None,
        timing: None,
        entry_date: "2026-10-28".parse().unwrap(),
        description: "Local work".into(),
        original_seconds: 780,
        recorded_settings: Some(RecordedSettings {
            rounding_interval_minutes: Some(5),
        }),
    };
    let first = Transfer {
        operation_id: Uuid::new_v4(),
        entry_id: entry.entry_id,
        change: Change::Create(entry.clone()),
    };
    queue.stage_transfer(&first).unwrap();
    queue.confirm(first.operation_id).unwrap();
    let transfer = Transfer {
        operation_id: Uuid::new_v4(),
        entry_id: entry.entry_id,
        change: Change::Replace(entry.clone()),
    };
    queue.stage_transfer(&transfer).unwrap();
    queue.mark_conflict(transfer.operation_id).unwrap();
    let remote = RemoteChange {
        applied_settings: None,
        entry_id: entry.entry_id,
        version: 2,
        entry: Some(Entry {
            description: "API work".into(),
            ..entry
        }),
    };
    queue
        .record_native_conflicts(std::slice::from_ref(&remote))
        .unwrap();
    (queue, transfer, remote)
}

#[test]
fn native_local_choice_keeps_original_transfer_until_verified_retry() {
    let (mut queue, transfer, remote) = fixture();
    queue
        .resolve_native(transfer.operation_id, remote.version, Choice::Local)
        .unwrap();
    let retry = queue.pending().unwrap()[0].operation_id;
    assert_ne!(retry, transfer.operation_id);
    assert!(!queue.stage_transfer(&transfer).unwrap());
    queue.confirm(retry).unwrap();
    assert!(queue.stage_transfer(&transfer).unwrap());
    assert_eq!(
        queue
            .native_resolution(transfer.operation_id)
            .unwrap()
            .unwrap()
            .choice,
        Choice::Local
    );
}

#[test]
fn native_remote_choice_is_durable_and_does_not_import_unrelated_entries() {
    let (mut queue, transfer, remote) = fixture();
    assert!(
        queue
            .resolve_native(transfer.operation_id, 1, Choice::Remote)
            .is_err()
    );
    let mut other = remote.clone();
    other.entry_id = Uuid::new_v4();
    other.entry.as_mut().unwrap().entry_id = other.entry_id;
    queue.record_native_conflicts(&[other.clone()]).unwrap();
    assert!(queue.entry(other.entry_id).unwrap().is_none());
    queue
        .resolve_native(transfer.operation_id, 2, Choice::Remote)
        .unwrap();
    let mut queue = Queue::prepare(queue.connection, &queue.scope.clone()).unwrap();
    assert!(queue.stage_transfer(&transfer).unwrap());
    assert_eq!(
        queue
            .native_resolution(transfer.operation_id)
            .unwrap()
            .unwrap()
            .remote
            .version,
        2
    );
    assert!(queue.pending().unwrap().is_empty());
}

#[test]
fn native_resolution_rejects_an_older_baseline_and_rolls_back_failed_choices() {
    let (mut queue, transfer, mut remote) = fixture();
    queue
        .connection
        .execute("DELETE FROM remote_conflicts", [])
        .unwrap();
    remote.version = 1;
    queue.record_native_conflicts(&[remote.clone()]).unwrap();
    assert!(
        queue
            .resolve_native(transfer.operation_id, 1, Choice::Local)
            .is_err()
    );
    remote.version = 2;
    queue.record_native_conflicts(&[remote]).unwrap();
    queue.connection.execute_batch("CREATE TRIGGER fail_decision BEFORE INSERT ON native_resolutions BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        queue
            .resolve_native(transfer.operation_id, 2, Choice::Remote)
            .is_err()
    );
    assert!(!queue.stage_transfer(&transfer).unwrap());
    assert_eq!(queue.native_conflicts().unwrap().len(), 1);
}

#[test]
fn repeated_conflict_and_remote_deletion_keep_the_original_native_key() {
    let (mut queue, transfer, mut remote) = fixture();
    queue
        .resolve_native(transfer.operation_id, 2, Choice::Local)
        .unwrap();
    let retry = queue.pending().unwrap()[0].operation_id;
    queue.mark_conflict(retry).unwrap();
    remote.version = 3;
    remote.entry = None;
    queue.record_native_conflicts(&[remote]).unwrap();
    assert_eq!(
        queue.native_conflicts().unwrap()[0].operation_id,
        transfer.operation_id
    );
    assert!(
        queue
            .resolve_native(transfer.operation_id, 3, Choice::Local)
            .is_err()
    );
    queue
        .resolve_native(transfer.operation_id, 3, Choice::Copy)
        .unwrap();
    assert!(queue.stage_transfer(&transfer).unwrap());
    assert!(
        queue
            .native_resolution(transfer.operation_id)
            .unwrap()
            .unwrap()
            .remote
            .entry
            .is_none()
    );
}
