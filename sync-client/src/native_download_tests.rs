use crate::native_download::RemoteOutcome;
use crate::transport::{ChangePage, RemoteChange};
use crate::*;

fn queue() -> Queue {
    Queue::prepare(
        rusqlite::Connection::open_in_memory().unwrap(),
        &Scope {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: Uuid::new_v4(),
            person_id: Uuid::new_v4(),
        },
    )
    .unwrap()
}
fn change() -> RemoteChange {
    let id = Uuid::new_v4();
    RemoteChange {
        applied_settings: None,
        entry_id: id,
        version: 1,
        entry: Some(Entry {
            entry_id: id,
            entry_date: "2026-10-29".parse().unwrap(),
            description: "Remote work".into(),
            original_seconds: 780,
            project_id: None,
            timing: None,
            recorded_settings: Some(RecordedSettings {
                rounding_interval_minutes: Some(5),
            }),
        }),
    }
}

#[test]
fn downloaded_batch_keeps_its_cursor_until_native_acknowledgement() {
    let mut q = queue();
    let change = change();
    let cursor = Uuid::new_v4();
    q.stage_native_page(
        None,
        ChangePage {
            changes: vec![change.clone()],
            cursor,
            more: false,
        },
    )
    .unwrap();
    let incoming = q.next_native_remote().unwrap().unwrap();
    assert_eq!(q.native_download_cursor().unwrap(), None);
    let mut q = Queue::prepare(q.connection, &q.scope.clone()).unwrap();
    assert_eq!(q.next_native_remote().unwrap().unwrap().id, incoming.id);
    assert!(q.entry(change.entry_id).unwrap().is_none());
    q.acknowledge_native_remote(&incoming, &RemoteOutcome::Applied)
        .unwrap();
    assert_eq!(q.native_download_cursor().unwrap(), Some(cursor));
    assert!(q.next_native_remote().unwrap().is_none());
    assert_eq!(q.entry(change.entry_id).unwrap(), change.entry);
}

#[test]
fn remote_conflict_ack_keeps_local_payload_and_does_not_advance_its_baseline() {
    use crate::transfer::{Change, Transfer};
    let mut q = queue();
    let mut change = change();
    let original = change.entry.clone().unwrap();
    let create = Transfer {
        operation_id: Uuid::new_v4(),
        entry_id: change.entry_id,
        change: Change::Create(original.clone()),
    };
    q.stage_transfer(&create).unwrap();
    q.confirm(create.operation_id).unwrap();
    change.version = 2;
    change.entry.as_mut().unwrap().description = "Other client".into();
    q.stage_native_page(
        None,
        ChangePage {
            changes: vec![change],
            cursor: Uuid::new_v4(),
            more: false,
        },
    )
    .unwrap();
    let incoming = q.next_native_remote().unwrap().unwrap();
    let edit = Transfer {
        operation_id: Uuid::new_v4(),
        entry_id: original.entry_id,
        change: Change::Replace(Entry {
            description: "Local edit".into(),
            ..original
        }),
    };
    q.acknowledge_native_remote(&incoming, &RemoteOutcome::Conflict(edit.clone()))
        .unwrap();
    assert_eq!(
        q.native_conflicts().unwrap()[0].operation_id,
        edit.operation_id
    );
    assert_eq!(
        q.entry(edit.entry_id).unwrap().unwrap().description,
        "Local edit"
    );
    let expected: i64 = q
        .connection
        .query_row(
            "SELECT expected_version FROM pending WHERE operation_id=?1",
            [edit.operation_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(expected, 1);
}

#[test]
fn bounded_pages_reject_replacement_and_advance_only_after_the_last_ack() {
    let mut q = queue();
    let cursor = Uuid::new_v4();
    assert!(
        q.stage_native_page(
            None,
            ChangePage {
                changes: (0..101).map(|_| change()).collect(),
                cursor,
                more: true
            }
        )
        .is_err()
    );
    q.stage_native_page(
        None,
        ChangePage {
            changes: (0..100).map(|_| change()).collect(),
            cursor,
            more: true,
        },
    )
    .unwrap();
    assert!(
        q.stage_native_page(
            None,
            ChangePage {
                changes: vec![change()],
                cursor: Uuid::new_v4(),
                more: false
            }
        )
        .is_err()
    );
    for _ in 0..100 {
        assert_eq!(q.native_download_cursor().unwrap(), None);
        let incoming = q.next_native_remote().unwrap().unwrap();
        assert!(
            q.acknowledge_native_remote(&incoming, &RemoteOutcome::Applied)
                .unwrap()
        );
    }
    assert_eq!(q.native_download_cursor().unwrap(), Some(cursor));
    assert!(q.next_native_remote().unwrap().is_none());
    let last = Uuid::new_v4();
    q.stage_native_page(
        Some(cursor),
        ChangePage {
            changes: vec![change()],
            cursor: last,
            more: false,
        },
    )
    .unwrap();
    let incoming = q.next_native_remote().unwrap().unwrap();
    assert!(
        !q.acknowledge_native_remote(&incoming, &RemoteOutcome::Applied)
            .unwrap()
    );
    assert!(
        !q.acknowledge_native_remote(&incoming, &RemoteOutcome::Applied)
            .unwrap()
    );
    assert_eq!(q.native_download_cursor().unwrap(), Some(last));
}

#[test]
fn a_confirmed_local_version_skips_its_feed_echo_but_keeps_the_next_edit() {
    use crate::transfer::{Change, Transfer};
    let mut q = queue();
    let mut remote = change();
    let transfer = Transfer {
        operation_id: Uuid::new_v4(),
        entry_id: remote.entry_id,
        change: Change::Create(remote.entry.clone().unwrap()),
    };
    q.stage_transfer(&transfer).unwrap();
    q.confirm(transfer.operation_id).unwrap();
    let echo = remote.clone();
    remote.version = 2;
    remote.entry.as_mut().unwrap().description = "New remote edit".into();
    let cursor = Uuid::new_v4();
    q.stage_native_page(
        None,
        ChangePage {
            changes: vec![echo, remote.clone()],
            cursor,
            more: false,
        },
    )
    .unwrap();
    assert_eq!(q.next_native_remote().unwrap().unwrap().change, remote);
    assert_eq!(q.native_download_cursor().unwrap(), None);
}
