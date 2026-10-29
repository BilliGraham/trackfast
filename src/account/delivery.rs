use super::*;
use std::time::{Duration, Instant};
use trackfast_sync::{Queue, scheduler, transfer::Transfer};

#[derive(Default)]
pub(super) struct Delivery {
    active: bool,
    waiting: bool,
    next: Option<Instant>,
    queue: Option<Queue>,
    last_operation: Option<uuid::Uuid>,
    resume: bool,
    review_cursor: Option<uuid::Uuid>,
}
impl Delivery {
    fn open_queue(&mut self, scope: &Scope) -> Result<&mut Queue, String> {
        if self.queue.is_none() {
            let path = crate::db::default_path()
                .ok_or("The local data folder is unavailable.")?
                .with_file_name("native-sync-queue.db");
            self.queue = Some(Queue::open(&path, scope).map_err(|e| e.to_string())?);
        }
        Ok(self.queue.as_mut().unwrap())
    }

    pub fn review(
        &mut self,
        config: &Config,
        client: &mut Option<SessionClient>,
        scope: &Scope,
        notify: &impl Fn(Event),
    ) {
        self.stop(notify);
        let result = (|| -> Result<(), String> {
            if scope != &config.scope() {
                return Err("The conflict account does not match this connection.".into());
            }
            restore(client, config).map_err(|e| e.to_string())?;
            let cursor = self.review_cursor;
            let queue = self.open_queue(scope)?;
            if queue
                .native_conflicts()
                .map_err(|e| e.to_string())?
                .is_empty()
            {
                notify(Event::Conflicts(scope.clone(), vec![], false));
                return Ok(());
            }
            let page = client
                .as_mut()
                .unwrap()
                .read_changes(cursor)
                .map_err(|e| e.to_string())?;
            queue
                .record_native_conflicts(&page.changes)
                .map_err(|e| e.to_string())?;
            let conflicts = queue.native_conflicts().map_err(|e| e.to_string())?;
            self.review_cursor = page.more.then_some(page.cursor);
            notify(Event::Conflicts(scope.clone(), conflicts, page.more));
            Ok(())
        })();
        if let Err(error) = result {
            self.review_cursor = None;
            notify(Event::ConflictError(error));
        }
    }

    pub fn resolve(
        &mut self,
        config: &Config,
        scope: &Scope,
        operation: uuid::Uuid,
        version: i64,
        choice: trackfast_sync::native_resolution::Choice,
        notify: &impl Fn(Event),
    ) {
        self.stop(notify);
        let result = (|| -> Result<(), String> {
            if scope != &config.scope() || self.review_cursor.is_some() {
                return Err("Finish loading the conflict before choosing a version.".into());
            }
            self.open_queue(scope)?
                .resolve_native(operation, version, choice)
                .map_err(|e| e.to_string())?;
            notify(Event::Conflicts(scope.clone(), vec![], false));
            self.start(notify);
            Ok(())
        })();
        if let Err(error) = result {
            notify(Event::ConflictError(error));
        }
    }
    pub fn start(&mut self, notify: &impl Fn(Event)) {
        self.active = true;
        self.resume = true;
        self.next = None;
        notify(Event::DeliveryState(
            true,
            "Delivery started. New mapped work will be sent. API changes will be received.".into(),
        ));
    }
    pub fn stop(&mut self, notify: &impl Fn(Event)) {
        self.active = false;
        self.waiting = false;
        notify(Event::DeliveryState(
            false,
            "Delivery stopped. Pending work stays on this computer.".into(),
        ));
    }
    pub fn confirmed(&mut self) {
        self.waiting = false;
        self.next = None;
    }
    pub fn request(&mut self, config: &Config, notify: &impl Fn(Event)) {
        if self.active && !self.waiting && self.next.is_none_or(|next| Instant::now() >= next) {
            self.waiting = true;
            notify(Event::InspectDelivery(config.scope()));
        }
    }
    pub fn inspected(
        &mut self,
        config: &Config,
        client: &mut Option<SessionClient>,
        scope: &Scope,
        transfer: Option<&Transfer>,
        notify: &impl Fn(Event),
    ) {
        if !self.active {
            self.waiting = false;
            return;
        }
        if transfer.is_some() {
            self.deliver(config, client, scope, transfer, notify);
            return;
        }
        let result = (|| -> Result<_, String> {
            if scope != &config.scope() {
                return Err("The download account does not match this connection.".into());
            }
            self.open_queue(scope)?
                .next_native_remote()
                .map_err(|e| e.to_string())
        })();
        match result {
            Ok(Some(incoming)) => notify(Event::ApplyRemote(scope.clone(), Box::new(incoming))),
            Ok(None) => notify(Event::RequestDelivery(scope.clone())),
            Err(error) => {
                self.waiting = false;
                self.active = false;
                notify(Event::DeliveryState(false, error));
            }
        }
    }
    pub fn remote_applied(
        &mut self,
        config: &Config,
        scope: &Scope,
        incoming: &trackfast_sync::native_download::Incoming,
        outcome: &trackfast_sync::native_download::RemoteOutcome,
        notify: &impl Fn(Event),
    ) {
        self.waiting = false;
        let result = (|| -> Result<(), String> {
            if scope != &config.scope() {
                return Err("The download account does not match this connection.".into());
            }
            self.open_queue(scope)?
                .acknowledge_native_remote(incoming, outcome)
                .map_err(|e| e.to_string())?;
            self.next = None;
            if matches!(
                outcome,
                trackfast_sync::native_download::RemoteOutcome::Conflict(_)
            ) {
                return Err(pause_message(scheduler::Pause::Conflict));
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.active = false;
            notify(Event::DeliveryState(false, error));
        }
    }
    pub fn deliver(
        &mut self,
        config: &Config,
        client: &mut Option<SessionClient>,
        scope: &Scope,
        transfer: Option<&Transfer>,
        notify: &impl Fn(Event),
    ) {
        self.waiting = false;
        if !self.active {
            return;
        }
        let result = (|| -> Result<(), String> {
            if scope != &config.scope() {
                return Err("The delivery account does not match the saved setup.".into());
            }
            let Some(transfer) = transfer else {
                let resume = self.resume;
                let queue = self.open_queue(scope)?;
                if resume {
                    queue
                        .resume_sync(jiff::Timestamp::now().as_second().saturating_mul(1000))
                        .map_err(|e| e.to_string())?;
                }
                if client.is_none() {
                    *client = Some(SessionClient::local(scope.clone()).map_err(|e| e.to_string())?);
                }
                let schedule = scheduler::download_step(queue, client.as_mut().unwrap())
                    .map_err(|e| e.to_string())?;
                let pending = queue
                    .next_native_remote()
                    .map_err(|e| e.to_string())?
                    .is_some();
                self.resume = false;
                if let Some(pause) = schedule.pause {
                    return Err(pause_message(pause));
                }
                let delay = (schedule.next_attempt_ms
                    - jiff::Timestamp::now().as_second().saturating_mul(1000))
                .clamp(1000, 300000) as u64;
                self.next = if pending {
                    None
                } else {
                    Some(Instant::now() + Duration::from_millis(delay.min(5000)))
                };
                notify(Event::DeliveryState(true,if pending {"Applying downloaded API changes."} else if schedule.failures>0 {"The API is unavailable. Local work is preserved. Sync will retry."} else {"Available mapped changes are confirmed. Sync checks for API changes while delivery is active."}.into()));
                return Ok(());
            };
            if self.queue.is_none() {
                let path = crate::db::default_path()
                    .ok_or("The local data folder is unavailable.")?
                    .with_file_name("native-sync-queue.db");
                self.queue = Some(Queue::open(&path, scope).map_err(|e| e.to_string())?);
            }
            let queue = self.queue.as_mut().unwrap();
            if self.resume || self.last_operation != Some(transfer.operation_id) {
                queue
                    .resume_sync(jiff::Timestamp::now().as_second().saturating_mul(1000))
                    .map_err(|e| e.to_string())?;
                self.resume = false;
                self.last_operation = Some(transfer.operation_id);
            }
            if !queue.stage_transfer(transfer).map_err(|e| e.to_string())? {
                if client.is_none() {
                    *client = Some(SessionClient::local(scope.clone()).map_err(|e| e.to_string())?);
                }
                let schedule = scheduler::delivery_step(queue, client.as_mut().unwrap())
                    .map_err(|e| e.to_string())?;
                if let Some(pause) = schedule.pause {
                    return Err(pause_message(pause));
                }
                if !queue.stage_transfer(transfer).map_err(|e| e.to_string())? {
                    let delay = (schedule.next_attempt_ms
                        - jiff::Timestamp::now().as_second().saturating_mul(1000))
                    .max(1000) as u64;
                    self.next = Some(Instant::now() + Duration::from_millis(delay.min(300000)));
                    notify(Event::DeliveryState(
                        true,
                        "Waiting for the next delivery attempt. Pending work is preserved.".into(),
                    ));
                    return Ok(());
                }
            }
            self.waiting = true;
            if let Some(resolution) = queue
                .native_resolution(transfer.operation_id)
                .map_err(|e| e.to_string())?
            {
                notify(Event::ApplyResolution(
                    scope.clone(),
                    transfer.operation_id,
                    Box::new(resolution),
                ));
            } else {
                notify(Event::ConfirmDelivery(scope.clone(), transfer.operation_id));
            }
            notify(Event::DeliveryState(
                true,
                "Saving the confirmed result locally...".into(),
            ));
            Ok(())
        })();
        if let Err(error) = result {
            self.active = false;
            if scope == &config.scope()
                && let Some(transfer) = transfer
            {
                notify(Event::DeliveryAttention(
                    scope.clone(),
                    transfer.operation_id,
                ));
            }
            notify(Event::DeliveryState(false, error));
        }
    }
}

fn pause_message(pause: scheduler::Pause) -> String {
    let reason = match pause {
        scheduler::Pause::Authentication => "Sign in again before restarting delivery.",
        scheduler::Pause::Conflict => "The API entry changed. Select Review conflicts in Settings.",
        scheduler::Pause::Rejected => {
            "The API rejected a pending change. That change needs review."
        }
        scheduler::Pause::Storage => "The credential store or queue is unavailable.",
        scheduler::Pause::InvalidResponse => "The API response failed validation.",
        scheduler::Pause::Configuration => "The API account or configuration does not match.",
    };
    format!("Delivery paused. {reason} Local work is preserved.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_frozen_receipt_finishes_before_an_unrelated_saved_download() {
        let config = Config {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: uuid::Uuid::new_v4(),
            person_id: uuid::Uuid::new_v4(),
            google_client_id: "test.apps.googleusercontent.com".into(),
        };
        let path = std::env::temp_dir().join(format!(
            "trackfast-receipt-first-{}.db",
            uuid::Uuid::new_v4()
        ));
        let mut queue = Queue::open(&path, &config.scope()).unwrap();
        let entry = trackfast_sync::Entry {
            entry_id: uuid::Uuid::new_v4(),
            entry_date: "2026-10-29".parse().unwrap(),
            description: "Confirmed work".into(),
            original_seconds: 600,
            project_id: None,
            timing: None,
            recorded_settings: None,
        };
        let transfer = Transfer {
            operation_id: uuid::Uuid::new_v4(),
            entry_id: entry.entry_id,
            change: trackfast_sync::transfer::Change::Create(entry),
        };
        queue.stage_transfer(&transfer).unwrap();
        queue.confirm(transfer.operation_id).unwrap();
        queue
            .stage_native_page(
                None,
                trackfast_sync::transport::ChangePage {
                    cursor: uuid::Uuid::new_v4(),
                    more: false,
                    changes: vec![trackfast_sync::transport::RemoteChange {
                        entry_id: uuid::Uuid::new_v4(),
                        version: 1,
                        entry: None,
                        applied_settings: None,
                    }],
                },
            )
            .unwrap();
        let mut delivery = Delivery {
            active: true,
            queue: Some(queue),
            ..Default::default()
        };
        let (tx, rx) = mpsc::channel();
        let notify = |event| tx.send(event).unwrap();
        delivery.inspected(
            &config,
            &mut None,
            &config.scope(),
            Some(&transfer),
            &notify,
        );
        assert!(
            matches!(rx.recv().unwrap(),Event::ConfirmDelivery(_,operation) if operation==transfer.operation_id)
        );
        assert!(
            delivery
                .queue
                .as_mut()
                .unwrap()
                .next_native_remote()
                .unwrap()
                .is_some()
        );
        assert!(delivery.waiting);
        drop(delivery);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn download_is_applied_before_new_outbound_work() {
        let config = Config {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: uuid::Uuid::new_v4(),
            person_id: uuid::Uuid::new_v4(),
            google_client_id: "test.apps.googleusercontent.com".into(),
        };
        let path =
            std::env::temp_dir().join(format!("trackfast-download-{}.db", uuid::Uuid::new_v4()));
        let mut queue = Queue::open(&path, &config.scope()).unwrap();
        queue
            .stage_native_page(
                None,
                trackfast_sync::transport::ChangePage {
                    cursor: uuid::Uuid::new_v4(),
                    more: false,
                    changes: vec![trackfast_sync::transport::RemoteChange {
                        applied_settings: None,
                        entry_id: uuid::Uuid::new_v4(),
                        version: 1,
                        entry: None,
                    }],
                },
            )
            .unwrap();
        let incoming = queue.next_native_remote().unwrap().unwrap();
        let mut delivery = Delivery {
            queue: Some(queue),
            ..Default::default()
        };
        let (tx, rx) = mpsc::channel();
        let notify = |event| tx.send(event).unwrap();
        delivery.start(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(matches!(rx.recv().unwrap(), Event::InspectDelivery(_)));
        delivery.inspected(&config, &mut None, &config.scope(), None, &notify);
        assert!(matches!(rx.recv().unwrap(),Event::ApplyRemote(_,got) if *got==incoming));
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.stop(&notify);
        rx.recv().unwrap();
        delivery.remote_applied(
            &config,
            &config.scope(),
            &incoming,
            &trackfast_sync::native_download::RemoteOutcome::Applied,
            &notify,
        );
        delivery.request(&config, &notify);
        assert!(!delivery.active);
        drop(delivery);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn stopped_delivery_does_not_request_work_and_can_restart_after_failure() {
        let mut delivery = Delivery::default();
        let config = Config {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: uuid::Uuid::new_v4(),
            person_id: uuid::Uuid::new_v4(),
            google_client_id: "test.apps.googleusercontent.com".into(),
        };
        let path =
            std::env::temp_dir().join(format!("trackfast-controller-{}.db", uuid::Uuid::new_v4()));
        delivery.queue = Some(Queue::open(&path, &config.scope()).unwrap());
        let (tx, rx) = mpsc::channel();
        let notify = |event| tx.send(event).unwrap();
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.start(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(matches!(rx.recv().unwrap(), Event::InspectDelivery(_)));
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.stop(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.start(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(matches!(rx.recv().unwrap(), Event::InspectDelivery(_)));
        drop(delivery);
        std::fs::remove_file(path).unwrap();
    }
}
