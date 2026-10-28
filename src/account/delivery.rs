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
}
impl Delivery {
    pub fn start(&mut self, notify: &impl Fn(Event)) {
        self.active = true;
        self.resume = true;
        self.next = None;
        notify(Event::DeliveryState(
            true,
            "Delivery started. New mapped work will be sent to Trackfast.".into(),
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
            notify(Event::RequestDelivery(config.scope()));
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
                self.next = Some(Instant::now() + Duration::from_secs(5));
                notify(Event::DeliveryState(
                    true,
                    "All available mapped changes are confirmed. Unmapped work stays local.".into(),
                ));
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
                    let reason = match pause {
                        scheduler::Pause::Authentication => {
                            "Sign in again before restarting delivery."
                        }
                        scheduler::Pause::Conflict => {
                            "The API entry changed. Conflict review is not available in the native app yet."
                        }
                        scheduler::Pause::Rejected => {
                            "The API rejected a pending change. That change needs review."
                        }
                        scheduler::Pause::Storage => {
                            "The credential store or queue is unavailable."
                        }
                        scheduler::Pause::InvalidResponse => "The API response failed validation.",
                        scheduler::Pause::Configuration => {
                            "The API account or configuration does not match."
                        }
                    };
                    return Err(format!(
                        "Delivery paused. {reason} Local work is preserved."
                    ));
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
            notify(Event::ConfirmDelivery(scope.clone(), transfer.operation_id));
            notify(Event::DeliveryState(
                true,
                "The API confirmed a change. Saving its receipt locally...".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_delivery_does_not_request_work_and_can_restart_after_failure() {
        let mut delivery = Delivery::default();
        let config = Config {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: uuid::Uuid::new_v4(),
            person_id: uuid::Uuid::new_v4(),
            google_client_id: "test.apps.googleusercontent.com".into(),
        };
        let (tx, rx) = mpsc::channel();
        let notify = |event| tx.send(event).unwrap();
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.start(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(matches!(rx.recv().unwrap(), Event::RequestDelivery(_)));
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.stop(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(rx.try_recv().is_err());
        delivery.start(&notify);
        rx.recv().unwrap();
        delivery.request(&config, &notify);
        assert!(matches!(rx.recv().unwrap(), Event::RequestDelivery(_)));
    }
}
