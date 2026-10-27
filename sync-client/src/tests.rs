use super::*;

#[test]
fn a_late_receipt_does_not_discard_a_newer_pulled_change() {
    let mut queue = queue();
    let local = entry();
    let key = queue.enqueue(&local).unwrap();
    let mut newer = local.clone();
    newer.description = "Changed on another client".into();
    queue
        .apply_remote_page(
            None,
            Uuid::new_v4(),
            &[crate::transport::RemoteChange {
                entry_id: local.entry_id,
                version: 2,
                entry: Some(newer.clone()),
            }],
        )
        .unwrap();
    queue.confirm(key).unwrap();
    assert_eq!(queue.entry(local.entry_id).unwrap(), Some(newer));
    assert!(queue.pending().unwrap().is_empty());
}

#[test]
fn remote_pages_keep_pending_work_and_commit_the_checkpoint_with_changes() {
    let mut queue = queue();
    let local = entry();
    let key = queue.enqueue(&local).unwrap();
    queue.confirm(key).unwrap();
    let mut edit = local.clone();
    edit.description = "Unsent work".into();
    let pending = queue.replace(&edit).unwrap();
    let checkpoint = Uuid::new_v4();
    let change = crate::transport::RemoteChange {
        entry_id: local.entry_id,
        version: 2,
        entry: None,
    };
    queue
        .apply_remote_page(None, checkpoint, &[change])
        .unwrap();
    assert_eq!(queue.entry(local.entry_id).unwrap(), Some(edit));
    assert_eq!(queue.pending().unwrap()[0].operation_id, pending);
    assert_eq!(queue.checkpoint().unwrap(), Some(checkpoint));
    assert!(
        queue
            .remote_conflict(local.entry_id)
            .unwrap()
            .unwrap()
            .entry
            .is_none()
    );
    let next = Uuid::new_v4();
    assert!(queue.apply_remote_page(None, next, &[]).is_err());
    assert_eq!(queue.checkpoint().unwrap(), Some(checkpoint));
    queue.connection.execute_batch("CREATE TRIGGER reject_checkpoint BEFORE UPDATE ON remote_checkpoint BEGIN SELECT RAISE(ABORT,'Test failure'); END;").unwrap();
    let remote = entry();
    let change = crate::transport::RemoteChange {
        entry_id: remote.entry_id,
        version: 1,
        entry: Some(remote.clone()),
    };
    assert!(
        queue
            .apply_remote_page(Some(checkpoint), next, &[change])
            .is_err()
    );
    assert!(queue.entry(remote.entry_id).unwrap().is_none());
    assert_eq!(queue.checkpoint().unwrap(), Some(checkpoint));
}

#[test]
fn timed_project_work_survives_restart_and_rejects_wrong_offsets() {
    let path = std::env::temp_dir().join(format!("timed-{}.db", Uuid::new_v4()));
    let owner = scope();
    let mut work = entry();
    work.project_id = Some(Uuid::new_v4());
    work.original_seconds = 780;
    work.timing = Some(Timing {
        started_at: "2026-10-26T09:03:00+02:00".into(),
        stopped_at: "2026-10-26T09:16:00+02:00".into(),
        time_zone: "Africa/Johannesburg".into(),
    });
    let key = Queue::open(&path, &owner).unwrap().enqueue(&work).unwrap();
    let mut reopened = Queue::open(&path, &owner).unwrap();
    assert_eq!(reopened.pending().unwrap()[0].entry, work);
    assert_eq!(reopened.pending().unwrap()[0].operation_id, key);
    work.entry_id = Uuid::new_v4();
    work.timing.as_mut().unwrap().started_at = "2026-10-26T09:03:00Z".into();
    assert!(reopened.enqueue(&work).is_err());
    assert_eq!(reopened.pending().unwrap().len(), 1);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}

fn entry() -> DurationEntry {
    DurationEntry {
        recorded_settings: None,
        project_id: None,
        timing: None,
        entry_id: Uuid::new_v4(),
        entry_date: NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(),
        description: "Test work".into(),
        original_seconds: 781,
    }
}

fn queue() -> Queue {
    Queue::prepare(Connection::open_in_memory().unwrap(), &scope()).unwrap()
}

#[test]
fn version_one_queue_upgrades_without_changing_pending_work() {
    let owner = scope();
    let original = entry();
    let key = Uuid::new_v4();
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(MIGRATIONS[0]).unwrap();
    connection
        .pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute(
            "INSERT INTO scope VALUES(1,?1)",
            [serde_json::to_string(&owner).unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO entries VALUES(?1,?2)",
            params![
                original.entry_id.to_string(),
                serde_json::to_string(&original).unwrap()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending(operation_id,entry_id) VALUES(?1,?2)",
            params![key.to_string(), original.entry_id.to_string()],
        )
        .unwrap();
    let mut queue = Queue::prepare(connection, &owner).unwrap();
    assert_eq!(queue.pending().unwrap()[0].operation_id, key);
    assert_eq!(queue.pending().unwrap()[0].entry, original);
    queue.confirm(key).unwrap();
    queue.replace(&original).unwrap();
}

#[test]
fn failed_confirmation_keeps_the_pending_operation_and_version() {
    let mut queue = queue();
    let original = entry();
    let key = queue.enqueue(&original).unwrap();
    queue.connection.execute_batch("CREATE TRIGGER reject_confirmation BEFORE DELETE ON pending BEGIN SELECT RAISE(ABORT, 'Test failure'); END;").unwrap();
    assert!(queue.confirm(key).is_err());
    assert_eq!(queue.pending().unwrap()[0].operation_id, key);
    let version: i64 = queue
        .connection
        .query_row("SELECT server_version FROM entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0);
}

#[test]
fn edits_and_deletion_keep_local_work_until_confirmation() {
    let mut queue = queue();
    let original = entry();
    let key = queue.enqueue(&original).unwrap();
    assert!(queue.replace(&original).is_err());
    queue.confirm(key).unwrap();
    let changed = DurationEntry {
        original_seconds: 10800,
        ..original.clone()
    };
    let edit = queue.replace(&changed).unwrap();
    assert_eq!(
        queue.entry(original.entry_id).unwrap(),
        Some(changed.clone())
    );
    assert!(queue.delete(original.entry_id).is_err());
    assert_eq!(queue.pending().unwrap()[0].operation_id, edit);
    queue.confirm(edit).unwrap();
    let deletion = queue.delete(original.entry_id).unwrap();
    assert_eq!(queue.entry(original.entry_id).unwrap(), Some(changed));
    queue.confirm(deletion).unwrap();
    assert_eq!(queue.entry(original.entry_id).unwrap(), None);
    assert!(queue.enqueue(&original).is_err());
}

#[test]
fn conflict_resolution_uses_a_new_key_and_keeps_the_local_values() {
    let mut queue = queue();
    let original = entry();
    let key = queue.enqueue(&original).unwrap();
    queue.confirm(key).unwrap();
    let changed = DurationEntry {
        original_seconds: 10800,
        ..original
    };
    let key = queue.replace(&changed).unwrap();
    assert!(queue.resolve_with_local(key, 2).is_err());
    queue
        .connection
        .execute(
            "UPDATE pending SET conflict=1 WHERE operation_id=?1",
            [key.to_string()],
        )
        .unwrap();
    assert!(queue.resolve_with_local(key, 1).is_err());
    let resolved = queue.resolve_with_local(key, 2).unwrap();
    assert_ne!(resolved, key);
    assert_eq!(queue.pending().unwrap()[0].operation_id, resolved);
    assert_eq!(queue.entry(changed.entry_id).unwrap(), Some(changed));
    assert_eq!(
        queue.operation_details(resolved).unwrap(),
        ("replace".into(), Some(2))
    );
}

#[test]
fn retry_and_restart_keep_exact_content_and_operation_key() {
    let path = std::env::temp_dir().join(format!("trackfast-queue-{}.db", Uuid::new_v4()));
    let owner = scope();
    let local = entry();
    let key;
    {
        let mut queue = Queue::open(&path, &owner).unwrap();
        key = queue.enqueue(&local).unwrap();
        let first = queue.pending().unwrap();
        assert_eq!(first, queue.pending().unwrap());
        assert_eq!(
            first[0],
            PendingCreate {
                operation_id: key,
                entry: local.clone()
            }
        );
    }
    {
        let mut queue = Queue::open(&path, &owner).unwrap();
        assert_eq!(queue.pending().unwrap()[0].operation_id, key);
        assert_eq!(queue.entry(local.entry_id).unwrap(), Some(local.clone()));
        queue.confirm(key).unwrap();
    }
    {
        let queue = Queue::open(&path, &owner).unwrap();
        assert!(queue.pending().unwrap().is_empty());
        assert_eq!(queue.entry(local.entry_id).unwrap(), Some(local));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_queue_insert_rolls_back_local_entry() {
    let mut queue = queue();
    queue.connection.execute_batch("CREATE TRIGGER reject_pending BEFORE INSERT ON pending BEGIN SELECT RAISE(ABORT, 'Test failure'); END;").unwrap();
    let local = entry();
    assert!(queue.enqueue(&local).is_err());
    assert_eq!(queue.entry(local.entry_id).unwrap(), None);
    assert!(queue.pending().unwrap().is_empty());
}

#[test]
fn different_account_or_server_cannot_open_queue() {
    let owner = scope();
    for changed in [
        Scope {
            person_id: Uuid::new_v4(),
            ..owner.clone()
        },
        Scope {
            tenant_id: Uuid::new_v4(),
            ..owner.clone()
        },
        Scope {
            server: "http://127.0.0.1:9000".into(),
            ..owner.clone()
        },
    ] {
        let queue = Queue::prepare(Connection::open_in_memory().unwrap(), &owner).unwrap();
        assert!(matches!(
            Queue::prepare(queue.connection, &changed),
            Err(Error::ScopeMismatch)
        ));
    }
}

#[test]
fn duplicate_entry_cannot_replace_pending_content() {
    let mut queue = queue();
    let local = entry();
    queue.enqueue(&local).unwrap();
    let before = queue.pending().unwrap();
    let mut changed = local.clone();
    changed.original_seconds = 10800;
    assert!(matches!(
        queue.enqueue(&changed),
        Err(Error::DuplicateEntry)
    ));
    assert_eq!(queue.pending().unwrap(), before);
    assert_eq!(queue.entry(local.entry_id).unwrap(), Some(local));
}

#[test]
fn unknown_confirmation_keeps_pending_work() {
    let mut queue = queue();
    queue.enqueue(&entry()).unwrap();
    let before = queue.pending().unwrap();
    assert!(matches!(
        queue.confirm(Uuid::new_v4()),
        Err(Error::UnknownOperation)
    ));
    assert_eq!(queue.pending().unwrap(), before);
}

#[test]
fn invalid_entries_do_not_enter_queue() {
    let mut queue = queue();
    for seconds in [0, 9_007_199_254_740_992] {
        let local = DurationEntry {
            original_seconds: seconds,
            ..entry()
        };
        assert!(matches!(queue.enqueue(&local), Err(Error::InvalidEntry)));
        assert_eq!(queue.entry(local.entry_id).unwrap(), None);
    }
    let local = DurationEntry {
        description: "x".repeat(2001),
        ..entry()
    };
    assert!(matches!(queue.enqueue(&local), Err(Error::InvalidEntry)));
    assert!(queue.pending().unwrap().is_empty());
}

#[test]
fn unrelated_or_newer_database_is_not_changed() {
    for sql in [
        "CREATE TABLE sessions(id INTEGER PRIMARY KEY); INSERT INTO sessions VALUES (7);",
        "PRAGMA application_id = 1413894993; PRAGMA user_version = 99;",
    ] {
        let path = std::env::temp_dir().join(format!("trackfast-foreign-{}.db", Uuid::new_v4()));
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(sql).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            Queue::open(&path, &scope()),
            Err(Error::IncompatibleDatabase)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn confirmation_removes_only_its_operation_and_keeps_three_hours_intact() {
    let mut queue = queue();
    let first = entry();
    let second = DurationEntry {
        original_seconds: 10800,
        ..entry()
    };
    let first_key = queue.enqueue(&first).unwrap();
    let second_key = queue.enqueue(&second).unwrap();
    queue.confirm(first_key).unwrap();
    assert_eq!(
        queue.pending().unwrap(),
        vec![PendingCreate {
            operation_id: second_key,
            entry: second
        }]
    );
    assert_eq!(queue.entry(first.entry_id).unwrap(), Some(first));
    assert!(matches!(
        queue.confirm(first_key),
        Err(Error::UnknownOperation)
    ));
}
