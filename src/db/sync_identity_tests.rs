use super::*;

fn identity(store: &Store, session: i64) -> (i64, String, i64) {
    store
        .connection
        .query_row(
            "SELECT sequence, entry_key, revision FROM session_sync_identity WHERE session_id=?1",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

fn insert(store: &Store, description: &str) -> i64 {
    store
        .connection
        .execute(
            "INSERT INTO sessions(description,start_utc,stop_utc) VALUES (?1,0,60)",
            [description],
        )
        .unwrap();
    store.connection.last_insert_rowid()
}

#[test]
fn migration_preserves_old_entries_and_stable_identity_after_reopen() {
    let connection = Connection::open_in_memory().unwrap();
    for step in &MIGRATIONS[..2] {
        connection.execute_batch(step).unwrap();
    }
    connection
        .execute(
            "INSERT INTO sessions(description,start_utc,stop_utc) VALUES ('Old',0,60)",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    let store = Store::prepare(connection).unwrap();
    let before = identity(&store, 1);
    assert_eq!(before.2, 1);
    assert!(uuid::Uuid::parse_str(&before.1).is_ok());
    let store = Store::prepare(store.connection).unwrap();
    assert_eq!(identity(&store, 1), before);
    assert_eq!(
        store.snapshot(Timestamp::UNIX_EPOCH).unwrap().sessions[0].description,
        "Old"
    );
}

#[test]
fn deletion_keeps_identity_and_reused_row_id_gets_a_new_identity() {
    let mut store = Store::open_in_memory().unwrap();
    let id = insert(&store, "First");
    let first = identity(&store, id);
    store.delete_entry(id).unwrap();
    let deleted: (Option<i64>, i64) = store
        .connection
        .query_row(
            "SELECT session_id, revision FROM session_sync_identity WHERE entry_key=?1",
            [&first.1],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(deleted, (None, 2));
    let replacement = insert(&store, "Second");
    assert_eq!(replacement, id);
    let second = identity(&store, replacement);
    assert_ne!(first.1, second.1);
    assert!(second.0 > first.0);
}

#[test]
fn edits_increment_revision_in_the_same_transaction() {
    let mut store = Store::open_in_memory().unwrap();
    let id = insert(&store, "First");
    let first = identity(&store, id);
    {
        let tx = store.connection.transaction().unwrap();
        tx.execute(
            "UPDATE sessions SET description='Rollback' WHERE id=?1",
            [id],
        )
        .unwrap();
    }
    assert_eq!(identity(&store, id), first);
    store
        .connection
        .execute(
            "UPDATE sessions SET description='Changed' WHERE id=?1",
            [id],
        )
        .unwrap();
    let updated = identity(&store, id);
    assert_eq!(updated, (first.0, first.1, 2));
    store
        .connection
        .execute(
            "UPDATE sessions SET description=description WHERE id=?1",
            [id],
        )
        .unwrap();
    assert_eq!(identity(&store, id), updated);
}

#[test]
fn creation_order_does_not_depend_on_the_recorded_date() {
    let store = Store::open_in_memory().unwrap();
    let old = insert(&store, "Existing");
    let boundary = identity(&store, old).0;
    let new = insert(&store, "New backdated work");
    assert!(identity(&store, new).0 > boundary);
    store
        .connection
        .execute("UPDATE sessions SET start_utc=-3600 WHERE id=?1", [old])
        .unwrap();
    assert_eq!(identity(&store, old).0, boundary);
}

#[test]
fn stopping_a_timer_keeps_its_identity_and_recorded_rounding() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .add_task(
            &Task {
                description: "Work".into(),
                project_id: None,
            },
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    let session = snapshot.sessions[0].id;
    let before = identity(&store, session);
    store
        .save_settings(&crate::entries::Settings {
            round_to_five: false,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(identity(&store, session), before);
    store
        .pause(snapshot.rows[0].id, Timestamp::from_second(780).unwrap())
        .unwrap();
    let after = identity(&store, session);
    assert_eq!(after, (before.0, before.1, before.2 + 1));
    let snapshot = store.snapshot(Timestamp::UNIX_EPOCH).unwrap();
    assert!(snapshot.sessions[0].details.round_to_five);
}

#[test]
fn failed_identity_write_rolls_back_the_entry_change() {
    let store = Store::open_in_memory().unwrap();
    let id = insert(&store, "Original");
    store
        .connection
        .execute(
            "UPDATE session_sync_identity SET revision=9223372036854775807 WHERE session_id=?1",
            [id],
        )
        .unwrap();
    assert!(
        store
            .connection
            .execute(
                "UPDATE sessions SET description='Must rollback' WHERE id=?1",
                [id]
            )
            .is_err()
    );
    let description: String = store
        .connection
        .query_row("SELECT description FROM sessions WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(description, "Original");
}

#[test]
fn file_reopen_preserves_deleted_identity_and_creation_order() {
    let path = std::env::temp_dir().join(format!("trackfast-identity-{}.db", uuid::Uuid::new_v4()));
    let first = {
        let mut store = Store::open(&path).unwrap();
        let id = insert(&store, "Before restart");
        let first = identity(&store, id);
        store.delete_entry(id).unwrap();
        first
    };
    {
        let store = Store::open(&path).unwrap();
        let deleted: bool = store.connection.query_row(
            "SELECT session_id IS NULL AND revision=2 FROM session_sync_identity WHERE entry_key=?1",
            [&first.1], |r| r.get(0),
        ).unwrap();
        assert!(deleted);
        let new = insert(&store, "After restart");
        let next = identity(&store, new);
        assert!(next.0 > first.0);
        assert_ne!(next.1, first.1);
    }
    std::fs::remove_file(path).unwrap();
}
