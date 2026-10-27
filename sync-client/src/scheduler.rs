#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scope, session::SessionError, transport::SyncError};
    struct Backend {
        calls: usize,
        fail: bool,
        auth: bool,
        limited: bool,
    }
    impl SyncBackend for Backend {
        fn pull_changes(&mut self, _: &mut Queue) -> Result<bool, SessionError> {
            self.calls += 1;
            if self.auth {
                Err(SessionError::SignInRequired)
            } else if self.limited {
                Err(SessionError::Sync(SyncError::RateLimited {
                    retry_after_seconds: Some(60),
                }))
            } else if self.fail {
                Err(SessionError::Sync(SyncError::Unavailable))
            } else {
                Ok(false)
            }
        }
        fn sync(&mut self, _: &mut Queue) -> Result<usize, SessionError> {
            Ok(0)
        }
    }
    fn queue() -> Queue {
        Queue::prepare(
            rusqlite::Connection::open_in_memory().unwrap(),
            &Scope {
                server: "http://127.0.0.1:8788".into(),
                tenant_id: uuid::Uuid::new_v4(),
                person_id: uuid::Uuid::new_v4(),
            },
        )
        .unwrap()
    }
    fn backend() -> Backend {
        Backend {
            calls: 0,
            fail: true,
            auth: false,
            limited: false,
        }
    }
    #[test]
    fn pending_work_schedules_another_bounded_cycle() {
        let mut q = queue();
        q.enqueue(&crate::Entry {
            entry_id: uuid::Uuid::new_v4(),
            entry_date: "2026-10-27".parse().unwrap(),
            description: "Pending work".into(),
            original_seconds: 600,
            project_id: None,
            timing: None,
        })
        .unwrap();
        let mut b = backend();
        b.fail = false;
        step(&mut q, &mut b, || 1000, 0).unwrap();
        assert_eq!(q.sync_schedule().unwrap().next_attempt_ms, 1100);
    }
    #[test]
    fn transient_retries_wait_grow_and_reset_after_success() {
        let mut q = queue();
        let mut b = backend();
        step(&mut q, &mut b, || 1000, 0).unwrap();
        assert_eq!(q.sync_schedule().unwrap().next_attempt_ms, 6000);
        step(&mut q, &mut b, || 5999, 0).unwrap();
        assert_eq!(b.calls, 1);
        step(&mut q, &mut b, || 6000, 0).unwrap();
        assert_eq!(q.sync_schedule().unwrap().next_attempt_ms, 16000);
        b.fail = false;
        step(&mut q, &mut b, || 16000, 0).unwrap();
        assert_eq!(q.sync_schedule().unwrap().failures, 0);
        assert_eq!(q.sync_schedule().unwrap().next_attempt_ms, 46000);
        for failures in 1..100 {
            assert!((5000..=300000).contains(&backoff(failures, u64::MAX)));
        }
    }
    #[test]
    fn authentication_pauses_until_explicit_resume_and_rate_limits_cannot_be_bypassed() {
        let mut q = queue();
        let mut b = backend();
        b.auth = true;
        step(&mut q, &mut b, || 1000, 0).unwrap();
        assert_eq!(
            q.sync_schedule().unwrap().pause,
            Some(Pause::Authentication)
        );
        step(&mut q, &mut b, || 100000, 0).unwrap();
        assert_eq!(b.calls, 1);
        q.resume_sync(2000).unwrap();
        b.auth = false;
        b.limited = true;
        step(&mut q, &mut b, || 2000, 0).unwrap();
        q.resume_sync(3000).unwrap();
        step(&mut q, &mut b, || 3000, 0).unwrap();
        assert_eq!(b.calls, 2);
        assert_eq!(q.sync_schedule().unwrap().next_attempt_ms, 62000);
    }
    #[test]
    fn retry_state_survives_a_queue_restart() {
        let path =
            std::env::temp_dir().join(format!("trackfast-retry-{}.db", uuid::Uuid::new_v4()));
        let scope = queue().scope.clone();
        let mut q = Queue::open(&path, &scope).unwrap();
        let mut b = backend();
        step(&mut q, &mut b, || 1000, 0).unwrap();
        drop(q);
        let mut q = Queue::open(&path, &scope).unwrap();
        step(&mut q, &mut b, || 2000, 0).unwrap();
        assert_eq!(b.calls, 1);
        drop(q);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn worker_stops_without_another_request_during_backoff() {
        let q = queue();
        let b = backend();
        let (commands, rx) = std::sync::mpsc::channel();
        let (events, updates) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || run_with(q, b, rx, events));
        let event = updates
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(matches!(event, WorkerEvent::Schedule(_)));
        commands.send(Command::Stop).unwrap();
        assert!(worker.join().unwrap().is_ok());
    }
}
use crate::{
    Error, Queue,
    session::{SessionClient, SessionError},
    storage,
    transport::SyncError,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pause {
    Authentication,
    Conflict,
    Rejected,
    Storage,
    InvalidResponse,
    Configuration,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub failures: u32,
    pub next_attempt_ms: i64,
    pub not_before_ms: i64,
    pub pause: Option<Pause>,
}
#[derive(Debug)]
pub enum Command {
    Resume,
    Stop,
}
#[derive(Debug)]
pub enum WorkerEvent {
    Schedule(Schedule),
}

impl Queue {
    pub fn sync_schedule(&self) -> Result<Schedule, Error> {
        let raw: String = self
            .connection
            .query_row("SELECT state FROM sync_schedule WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        let schedule: Schedule = serde_json::from_str(&raw).map_err(storage)?;
        if schedule.failures > 31 || schedule.next_attempt_ms < 0 || schedule.not_before_ms < 0 {
            return Err(Error::Storage);
        }
        Ok(schedule)
    }
    fn save_schedule(&self, schedule: &Schedule) -> Result<(), Error> {
        self.connection
            .execute(
                "UPDATE sync_schedule SET state=?1 WHERE id=1",
                [serde_json::to_string(schedule).map_err(storage)?],
            )
            .map_err(storage)?;
        Ok(())
    }
    /// Resumes after explicit user action. A server retry deadline remains enforced.
    pub fn resume_sync(&mut self, now_ms: i64) -> Result<(), Error> {
        if now_ms < 0 {
            return Err(Error::Storage);
        }
        let mut schedule = self.sync_schedule()?;
        schedule.pause = None;
        schedule.failures = 0;
        schedule.next_attempt_ms = now_ms.max(schedule.not_before_ms);
        self.save_schedule(&schedule)
    }
}
trait SyncBackend {
    fn pull_changes(&mut self, queue: &mut Queue) -> Result<bool, SessionError>;
    fn sync(&mut self, queue: &mut Queue) -> Result<usize, SessionError>;
}
impl SyncBackend for SessionClient {
    fn pull_changes(&mut self, queue: &mut Queue) -> Result<bool, SessionError> {
        SessionClient::pull_changes(self, queue)
    }
    fn sync(&mut self, queue: &mut Queue) -> Result<usize, SessionError> {
        SessionClient::sync_batch(self, queue, 1)
    }
}
fn backoff(failures: u32, jitter: u64) -> i64 {
    let base = 5000u64.saturating_mul(1u64 << failures.saturating_sub(1).min(6));
    (base.saturating_add(jitter % (base / 5 + 1)).min(300000)) as i64
}
fn cycle(queue: &mut Queue, client: &mut impl SyncBackend) -> Result<bool, SessionError> {
    if !queue
        .has_conflicts()
        .map_err(|_| SessionError::Sync(SyncError::Storage))?
    {
        match client.sync(queue) {
            Ok(_) | Err(SessionError::Sync(SyncError::Conflict)) => {}
            Err(error) => return Err(error),
        }
    }
    let more = client.pull_changes(queue)?;
    if more {
        return Ok(true);
    }
    if queue
        .has_conflicts()
        .map_err(|_| SessionError::Sync(SyncError::Storage))?
    {
        return Err(SessionError::Sync(SyncError::Conflict));
    }
    Ok(!queue
        .pending()
        .map_err(|_| SessionError::Sync(SyncError::Storage))?
        .is_empty())
}
fn step(
    queue: &mut Queue,
    client: &mut impl SyncBackend,
    clock: impl Fn() -> i64,
    jitter: u64,
) -> Result<Schedule, Error> {
    let now = clock();
    if now < 0 {
        return Err(Error::Storage);
    }
    let mut schedule = queue.sync_schedule()?;
    if schedule.pause.is_some() || now < schedule.next_attempt_ms {
        return Ok(schedule);
    }
    // A process interruption must not erase the interval before the next attempt.
    schedule.next_attempt_ms = now.saturating_add(30000);
    queue.save_schedule(&schedule)?;
    let result = cycle(queue, client);
    let finished = clock().max(now);
    match result {
        Ok(more) => {
            schedule.failures = 0;
            schedule.not_before_ms = 0;
            schedule.next_attempt_ms = finished.saturating_add(if more { 100 } else { 30000 });
        }
        Err(SessionError::Sync(SyncError::Unavailable)) => {
            schedule.failures = schedule.failures.saturating_add(1).min(31);
            schedule.next_attempt_ms = finished.saturating_add(backoff(schedule.failures, jitter));
        }
        Err(SessionError::Sync(SyncError::RateLimited {
            retry_after_seconds,
        })) => {
            schedule.failures = schedule.failures.saturating_add(1).min(31);
            let server_delay = retry_after_seconds
                .unwrap_or(0)
                .saturating_mul(1000)
                .min(i64::MAX as u64) as i64;
            schedule.next_attempt_ms =
                finished.saturating_add(backoff(schedule.failures, jitter).max(server_delay));
            schedule.not_before_ms = schedule.next_attempt_ms;
        }
        Err(error) => {
            schedule.pause = Some(match error {
                SessionError::Sync(SyncError::Conflict) => Pause::Conflict,
                SessionError::Sync(SyncError::Rejected) => Pause::Rejected,
                SessionError::CredentialStore
                | SessionError::Busy
                | SessionError::Sync(SyncError::Storage) => Pause::Storage,
                SessionError::InvalidResponse | SessionError::Sync(SyncError::InvalidReceipt) => {
                    Pause::InvalidResponse
                }
                SessionError::InvalidConfiguration
                | SessionError::IdentityMismatch
                | SessionError::Sync(
                    SyncError::IdentityMismatch | SyncError::InvalidConfiguration,
                ) => Pause::Configuration,
                _ => Pause::Authentication,
            })
        }
    }
    queue.save_schedule(&schedule)?;
    Ok(schedule)
}

/// Runs scheduled queue delivery on the caller's worker thread. This function blocks.
/// Send Stop or drop the command sender to stop. In-flight requests finish within transport timeouts.
/// Authentication, conflicts, and permanent errors require an explicit Resume command after correction.
pub fn run(
    queue: Queue,
    client: SessionClient,
    commands: Receiver<Command>,
    events: Sender<WorkerEvent>,
) -> Result<(), Error> {
    run_with(queue, client, commands, events)
}
fn run_with(
    mut queue: Queue,
    mut client: impl SyncBackend,
    commands: Receiver<Command>,
    events: Sender<WorkerEvent>,
) -> Result<(), Error> {
    loop {
        match commands.try_recv() {
            Ok(Command::Stop) | Err(TryRecvError::Disconnected) => return Ok(()),
            Ok(Command::Resume) => queue.resume_sync(chrono::Utc::now().timestamp_millis())?,
            Err(TryRecvError::Empty) => {}
        }
        let schedule = queue.sync_schedule()?;
        let now = chrono::Utc::now().timestamp_millis();
        if schedule.pause.is_none() && schedule.next_attempt_ms <= now {
            let random = uuid::Uuid::new_v4().as_u128() as u64;
            let schedule = step(
                &mut queue,
                &mut client,
                || chrono::Utc::now().timestamp_millis(),
                random,
            )?;
            if events.send(WorkerEvent::Schedule(schedule)).is_err() {
                return Ok(());
            }
            continue;
        }
        let command = if schedule.pause.is_some() {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            commands.recv_timeout(Duration::from_millis(
                schedule.next_attempt_ms.saturating_sub(now).clamp(1, 30000) as u64,
            ))
        };
        match command {
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Command::Resume) => queue.resume_sync(chrono::Utc::now().timestamp_millis())?,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}
