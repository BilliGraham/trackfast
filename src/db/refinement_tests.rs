use super::*;
use crate::entries::{EntryDetails, Settings};

fn at(s: &str) -> Timestamp {
    s.parse().unwrap()
}
fn draft(seconds: i64) -> Session {
    Session {
        id: 0,
        description: "Design".into(),
        project_id: None,
        start: at("2026-10-26T00:00:00Z"),
        stop: Some(at("2026-10-26T00:00:00Z")),
        details: EntryDetails {
            duration_seconds: Some(seconds),
            entry_date: Some("2026-10-26".parse().unwrap()),
            ..Default::default()
        },
    }
}
fn data(store: &Store) -> Snapshot {
    store.snapshot(Timestamp::UNIX_EPOCH).unwrap()
}
#[test]
fn settings_and_entries_keep_their_rounding_after_restart() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(data(&store).settings, Settings::default());
    store.save_entry(&draft(780)).unwrap();
    let settings = Settings {
        round_to_five: false,
        weekly_target_hours: 32.5,
        glass: true,
    };
    store.save_settings(&settings).unwrap();
    store.save_entry(&draft(781)).unwrap();
    let store = Store::prepare(store.connection).unwrap();
    let snapshot = data(&store);
    assert_eq!(snapshot.settings, settings);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .map(|s| s.effective(at("2026-10-27T00:00:00Z")).as_secs())
            .sum::<i64>(),
        1681
    );
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .map(|s| s.elapsed(at("2026-10-27T00:00:00Z")).as_secs())
            .sum::<i64>(),
        1561
    );
    assert!(
        crate::entries::week::day_blocks(
            &snapshot.sessions,
            "2026-10-26".parse().unwrap(),
            &jiff::tz::TimeZone::UTC,
            at("2026-10-27T00:00:00Z")
        )
        .is_empty()
    );
}
#[test]
fn invalid_entries_and_settings_do_not_change_storage() {
    let mut store = Store::open_in_memory().unwrap();
    for seconds in [0, -1] {
        assert!(store.save_entry(&draft(seconds)).is_err());
    }
    let mut entry = draft(10800);
    entry.project_id = Some(999);
    assert!(store.save_entry(&entry).is_err());
    for hours in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let settings = Settings {
            weekly_target_hours: hours,
            ..Default::default()
        };
        assert!(store.save_settings(&settings).is_err());
    }
    assert!(data(&store).sessions.is_empty());
    assert_eq!(data(&store).settings, Settings::default());
}
#[test]
fn old_entries_stay_exact_and_live_timers_capture_settings_at_start() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(MIGRATIONS[0]).unwrap();
    connection
        .execute(
            "INSERT INTO sessions(description,start_utc,stop_utc) VALUES ('Old',0,781)",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    let mut store = Store::prepare(connection).unwrap();
    assert_eq!(
        data(&store).sessions[0]
            .effective(Timestamp::UNIX_EPOCH)
            .as_secs(),
        781
    );
    store
        .add_task(
            &Task {
                description: "New".into(),
                project_id: None,
            },
            at("2026-10-26T09:03:00Z"),
        )
        .unwrap();
    let row = data(&store).rows[0].id;
    let settings = Settings {
        round_to_five: false,
        ..Default::default()
    };
    store.save_settings(&settings).unwrap();
    store.pause(row, at("2026-10-26T09:16:00Z")).unwrap();
    let snapshot = data(&store);
    let new = snapshot
        .sessions
        .iter()
        .find(|s| s.description == "New")
        .unwrap();
    assert_eq!(new.elapsed(Timestamp::UNIX_EPOCH).as_secs(), 780);
    assert_eq!(new.effective(Timestamp::UNIX_EPOCH).as_secs(), 900);
}
#[test]
fn editing_keeps_rounding_and_deletion_removes_only_the_selected_entry() {
    let mut store = Store::open_in_memory().unwrap();
    store.save_entry(&draft(30)).unwrap();
    store.save_entry(&draft(10800)).unwrap();
    let mut entry = data(&store)
        .sessions
        .into_iter()
        .find(|s| s.elapsed(Timestamp::UNIX_EPOCH).as_secs() == 30)
        .unwrap();
    assert_eq!(entry.effective(Timestamp::UNIX_EPOCH).as_secs(), 0);
    let settings = Settings {
        round_to_five: false,
        ..Default::default()
    };
    store.save_settings(&settings).unwrap();
    entry.details.duration_seconds = Some(150);
    entry.details.round_to_five = false;
    store.save_entry(&entry).unwrap();
    let saved = data(&store)
        .sessions
        .into_iter()
        .find(|s| s.id == entry.id)
        .unwrap();
    assert_eq!(saved.effective(Timestamp::UNIX_EPOCH).as_secs(), 300);
    store.delete_entry(entry.id).unwrap();
    assert_eq!(data(&store).sessions.len(), 1);
    assert_eq!(
        data(&store).sessions[0]
            .elapsed(Timestamp::UNIX_EPOCH)
            .as_secs(),
        10800
    );
}

#[test]
fn timed_entries_reject_fractional_seconds_and_wrong_dates() {
    let mut store = Store::open_in_memory().unwrap();
    let mut entry = draft(780);
    entry.details.duration_seconds = None;
    entry.start = at("2026-10-26T09:03:00.5Z");
    entry.stop = Some(at("2026-10-26T09:16:00Z"));
    entry.details.time_zone = Some("UTC".into());
    assert!(store.save_entry(&entry).is_err());
    entry.start = at("2026-10-26T09:03:00Z");
    entry.details.entry_date = Some("2026-10-25".parse().unwrap());
    assert!(store.save_entry(&entry).is_err());
    assert!(data(&store).sessions.is_empty());
}

#[test]
fn totals_keep_the_saved_date_when_the_display_zone_changes() {
    let mut store = Store::open_in_memory().unwrap();
    let mut entry = draft(780);
    entry.start = at("2026-10-25T22:00:00Z");
    entry.stop = Some(entry.start);
    entry.details.time_zone = Some("Africa/Johannesburg".into());
    store.save_entry(&entry).unwrap();
    let snapshot = data(&store);
    let zone = jiff::tz::TimeZone::UTC;
    let date = "2026-10-26".parse().unwrap();
    let total = crate::entries::summary::total_dates(
        &snapshot.sessions,
        date,
        date,
        &zone,
        at("2026-10-27T00:00:00Z"),
    );
    assert_eq!(total.as_secs(), 900);
    let day = crate::entries::summary::days(&snapshot.sessions, &zone, Timestamp::UNIX_EPOCH);
    assert_eq!(day[0].date, date);
}

#[test]
fn loading_a_week_includes_entries_with_an_earlier_utc_anchor() {
    let mut store = Store::open_in_memory().unwrap();
    let mut entry = draft(780);
    entry.start = at("2026-10-25T22:00:00Z");
    entry.stop = Some(entry.start);
    store.save_entry(&entry).unwrap();
    assert_eq!(
        store
            .snapshot(at("2026-10-26T00:00:00Z"))
            .unwrap()
            .sessions
            .len(),
        1
    );
}
