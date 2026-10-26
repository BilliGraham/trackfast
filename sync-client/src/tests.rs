use super::*;

fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}

fn entry() -> DurationEntry {
    DurationEntry {
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
