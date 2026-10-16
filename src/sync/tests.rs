use std::collections::VecDeque;

use super::*;
use crate::toggl::User;
use crate::toggl::quota::{SYNC_INTERVAL, WINDOW};

struct FakeBackend {
    saved: bool,
    fetches: usize,
    delete_error: bool,
    responses: VecDeque<Result<Snapshot, ApiError>>,
}

impl Backend for FakeBackend {
    fn load_token(&mut self) -> Result<Option<ApiToken>, String> {
        Ok(self
            .saved
            .then(|| ApiToken::new("synthetic-token").unwrap()))
    }

    fn save_token(&mut self, _: &ApiToken) -> Result<(), String> {
        self.saved = true;
        Ok(())
    }

    fn delete_token(&mut self) -> Result<(), String> {
        if self.delete_error {
            return Err("The test credential store is unavailable.".into());
        }
        self.saved = false;
        Ok(())
    }

    fn fetch(&mut self, _: &Client, _: &mut RequestQuota) -> Result<Snapshot, ApiError> {
        self.fetches += 1;
        self.responses.pop_front().expect("Supply a test response.")
    }
}

fn worker(response: Result<Snapshot, ApiError>) -> (Worker<FakeBackend>, Receiver<Event>) {
    let (events, rx) = mpsc::channel();
    (
        Worker {
            events,
            ctx: egui::Context::default(),
            backend: FakeBackend {
                saved: true,
                fetches: 0,
                delete_error: false,
                responses: VecDeque::from([response]),
            },
            client: None,
            quota: RequestQuota::default(),
        },
        rx,
    )
}

fn snapshot() -> Snapshot {
    Snapshot {
        user: User {
            fullname: None,
            beginning_of_week: 1,
        },
        workspaces: Vec::new(),
        projects: Vec::new(),
        entries: Vec::new(),
        fetched_at: "2026-10-16T13:00:00Z".parse().unwrap(),
    }
}

#[test]
fn rejected_saved_credentials_return_to_sign_in() {
    let (mut worker, events) = worker(Err(ApiError::InvalidToken));
    worker.restore();
    assert!(worker.client.is_none());
    assert!(!worker.backend.saved);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::SignedOut { problem: Some(_) }))
    );
    worker.handle(Command::Refresh);
    assert_eq!(worker.backend.fetches, 1);
}

#[test]
fn rejected_credentials_remain_signed_out_when_deletion_fails() {
    let (mut worker, events) = worker(Err(ApiError::InvalidToken));
    worker.backend.delete_error = true;
    worker.restore();
    assert!(worker.client.is_none());
    let problem = events
        .try_iter()
        .find_map(|event| match event {
            Event::SignedOut { problem } => problem,
            _ => None,
        })
        .unwrap();
    assert!(problem.contains("cannot remove the saved token"));
}

#[test]
fn rejected_refresh_discards_the_authenticated_client() {
    let (mut worker, events) = worker(Err(ApiError::InvalidToken));
    worker.client = Some(Client::new(&ApiToken::new("synthetic-token").unwrap()));
    worker.handle(Command::Refresh);
    assert!(worker.client.is_none());
    assert!(!worker.backend.saved);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::SignedOut { .. }))
    );
}

#[test]
fn network_failures_keep_credentials_for_a_later_retry() {
    let (mut worker, events) = worker(Err(ApiError::Network("Test network failure".into())));
    worker.restore();
    assert!(worker.client.is_some());
    assert!(worker.backend.saved);
    assert!(events.try_iter().any(|event| matches!(
        event,
        Event::Failed {
            signed_in: true,
            ..
        }
    )));
}

#[test]
fn restore_and_sign_in_share_the_worker_cooldown() {
    let (mut worker, events) = worker(Ok(snapshot()));
    worker.restore();
    worker.handle(Command::Refresh);
    worker.handle(Command::SignOut);
    worker.handle(Command::SignIn(
        ApiToken::new("another-synthetic-token").unwrap(),
    ));
    assert_eq!(worker.backend.fetches, 1);
    assert!(worker.client.is_none());
    assert!(events.try_iter().any(|event| matches!(
        event,
        Event::Failed {
            signed_in: false,
            ..
        }
    )));
}

#[test]
fn quota_responses_block_later_sign_ins_and_refreshes() {
    let start = Instant::now();
    let (mut worker, events) = worker(Err(ApiError::QuotaReached));
    worker.restore();
    let retry_at = events
        .try_iter()
        .find_map(|event| match event {
            Event::RequestLimit(retry_at) => retry_at,
            _ => None,
        })
        .unwrap();
    assert!(retry_at >= start + WINDOW);
    assert!(retry_at > start + SYNC_INTERVAL);
    worker.handle(Command::Refresh);
    worker.handle(Command::SignOut);
    worker.handle(Command::SignIn(ApiToken::new("synthetic-token").unwrap()));
    assert_eq!(worker.backend.fetches, 1);
}

#[test]
fn a_rejected_sign_in_does_not_save_the_candidate_token() {
    let (mut worker, events) = worker(Err(ApiError::InvalidToken));
    worker.backend.saved = false;
    worker.handle(Command::SignIn(ApiToken::new("synthetic-token").unwrap()));
    assert!(worker.client.is_none());
    assert!(!worker.backend.saved);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::SignedOut { problem: Some(_) }))
    );
}

#[test]
fn an_accepted_sign_in_saves_the_token_and_loads_the_snapshot() {
    let (mut worker, events) = worker(Ok(snapshot()));
    worker.backend.saved = false;
    worker.handle(Command::SignIn(ApiToken::new("synthetic-token").unwrap()));
    assert!(worker.client.is_some());
    assert!(worker.backend.saved);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Loaded { problem: None, .. }))
    );
}
