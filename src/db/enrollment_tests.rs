use super::*;
use trackfast_sync::Scope;
use uuid::Uuid;

fn scope() -> Scope {
    Scope {
        server: "http://127.0.0.1:8788".into(),
        tenant_id: Uuid::new_v4(),
        person_id: Uuid::new_v4(),
    }
}
fn insert(store: &Store, imported: bool) {
    store
        .connection
        .execute(
            "INSERT INTO sessions(description,start_utc,stop_utc,toggl_id) VALUES ('Work',0,60,?1)",
            [imported.then_some(100)],
        )
        .unwrap();
}

#[test]
fn enrollment_keeps_one_boundary_across_restarts_and_excludes_old_work() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    insert(&store, false);
    assert!(store.enrollment().unwrap().is_none());
    store.enroll(&scope).unwrap();
    let first = store.enrollment().unwrap().unwrap();
    assert_eq!(first.boundary, 1);
    assert_eq!(first.new_entries, 0);
    insert(&store, false);
    insert(&store, true);
    let mut store = Store::prepare(store.connection).unwrap();
    store.enroll(&scope).unwrap();
    let current = store.enrollment().unwrap().unwrap();
    assert_eq!(current.boundary, first.boundary);
    assert_eq!(current.scope, scope);
    assert_eq!(current.new_entries, 1);
    store
        .connection
        .execute(
            "UPDATE sessions SET description='Edited old history' WHERE id=1",
            [],
        )
        .unwrap();
    assert_eq!(store.enrollment().unwrap().unwrap().new_entries, 1);
}

#[test]
fn enrollment_rejects_another_account_or_server_without_changing_data() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    store.enroll(&scope).unwrap();
    for other in [
        Scope {
            person_id: Uuid::new_v4(),
            ..scope.clone()
        },
        Scope {
            tenant_id: Uuid::new_v4(),
            ..scope.clone()
        },
        Scope {
            server: "http://127.0.0.1:8789".into(),
            ..scope.clone()
        },
    ] {
        assert!(store.enroll(&other).is_err());
    }
    assert_eq!(store.enrollment().unwrap().unwrap().scope, scope);
}

#[test]
fn mapping_requires_enrollment_and_cannot_silently_reassign_a_project() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    let local = store
        .save_project(&ProjectDraft {
            name: "Local".into(),
            colour: "#abcdef".into(),
            ..Default::default()
        })
        .unwrap();
    let remote = Uuid::new_v4();
    assert!(store.map_project(&scope, local, remote).is_err());
    store.enroll(&scope).unwrap();
    assert!(
        store
            .map_project(
                &Scope {
                    person_id: Uuid::new_v4(),
                    ..scope.clone()
                },
                local,
                remote
            )
            .is_err()
    );
    assert!(store.map_project(&scope, local, Uuid::nil()).is_err());
    assert!(store.map_project(&scope, 999, remote).is_err());
    store.map_project(&scope, local, remote).unwrap();
    store.map_project(&scope, local, remote).unwrap();
    assert!(store.map_project(&scope, local, Uuid::new_v4()).is_err());
    let state = store.enrollment().unwrap().unwrap();
    assert_eq!(state.mappings, vec![(local, remote)]);
}

#[test]
fn running_work_at_enrollment_stays_local_and_new_timers_keep_the_boundary() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .add_task(
            &Task {
                description: "Already running".into(),
                project_id: None,
            },
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();
    store.enroll(&scope()).unwrap();
    store.pause(1, Timestamp::from_second(60).unwrap()).unwrap();
    assert_eq!(store.enrollment().unwrap().unwrap().new_entries, 0);
    store.play(1, Timestamp::from_second(120).unwrap()).unwrap();
    assert_eq!(store.enrollment().unwrap().unwrap().new_entries, 0);
    store
        .pause(1, Timestamp::from_second(180).unwrap())
        .unwrap();
    assert_eq!(store.enrollment().unwrap().unwrap().new_entries, 1);
}

#[test]
fn invalid_scope_or_archived_project_cannot_change_setup() {
    let mut store = Store::open_in_memory().unwrap();
    let scope = scope();
    for invalid in [
        Scope {
            person_id: Uuid::nil(),
            ..scope.clone()
        },
        Scope {
            server: "http://example.com:8788".into(),
            ..scope.clone()
        },
    ] {
        assert!(store.enroll(&invalid).is_err());
        assert!(store.enrollment().unwrap().is_none());
    }
    store.enroll(&scope).unwrap();
    let local = store
        .save_project(&ProjectDraft {
            name: "Archived".into(),
            colour: "#abcdef".into(),
            ..Default::default()
        })
        .unwrap();
    store.set_archived(local, true).unwrap();
    assert!(store.map_project(&scope, local, Uuid::new_v4()).is_err());
    assert!(store.enrollment().unwrap().unwrap().mappings.is_empty());
}

#[test]
fn file_reopen_preserves_binding_boundary_and_mappings() {
    let path = std::env::temp_dir().join(format!("trackfast-enrollment-{}.db", Uuid::new_v4()));
    let scope = scope();
    let remote = Uuid::new_v4();
    let local = {
        let mut store = Store::open(&path).unwrap();
        insert(&store, false);
        store.enroll(&scope).unwrap();
        let local = store
            .save_project(&ProjectDraft {
                name: "Mapped".into(),
                colour: "#abcdef".into(),
                ..Default::default()
            })
            .unwrap();
        store.map_project(&scope, local, remote).unwrap();
        insert(&store, false);
        local
    };
    {
        let mut store = Store::open(&path).unwrap();
        store.enroll(&scope).unwrap();
        let saved = store.enrollment().unwrap().unwrap();
        assert_eq!(saved.scope, scope);
        assert_eq!(saved.boundary, 1);
        assert_eq!(saved.new_entries, 1);
        assert_eq!(saved.mappings, vec![(local, remote)]);
    }
    std::fs::remove_file(path).unwrap();
}
