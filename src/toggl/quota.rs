use std::collections::VecDeque;
use std::time::{Duration, Instant};

const REQUESTS_PER_HOUR: usize = 30;
pub const WINDOW: Duration = Duration::from_secs(60 * 60);
pub const SYNC_INTERVAL: Duration = Duration::from_secs(4 * 60);

#[derive(Default)]
pub struct RequestQuota {
    requests: VecDeque<Instant>,
    sync_after: Option<Instant>,
    blocked_until: Option<Instant>,
}

impl RequestQuota {
    pub fn begin_sync(&mut self, now: Instant) -> Result<(), Instant> {
        if let Some(retry_at) = self.retry_at(now) {
            return Err(retry_at);
        }
        // A complete sync needs two requests from the shared hourly allowance.
        self.sync_after = Some(now + SYNC_INTERVAL);
        Ok(())
    }

    pub fn record_request(&mut self, now: Instant) -> Result<(), Instant> {
        while self
            .requests
            .front()
            .is_some_and(|sent| *sent + WINDOW <= now)
        {
            self.requests.pop_front();
        }
        if let Some(retry_at) = self.request_retry_at(now, 1) {
            return Err(retry_at);
        }
        self.requests.push_back(now);
        Ok(())
    }

    pub fn block_after_quota_error(&mut self, now: Instant) {
        self.blocked_until = Some(now + WINDOW);
    }

    pub fn retry_at(&self, now: Instant) -> Option<Instant> {
        self.sync_after
            .filter(|retry_at| *retry_at > now)
            .into_iter()
            .chain(self.request_retry_at(now, 2))
            .max()
    }

    fn request_retry_at(&self, now: Instant, requests_needed: usize) -> Option<Instant> {
        let mut active = self.requests.iter().filter(|sent| **sent + WINDOW > now);
        let count = active.clone().count();
        let budget_retry = (count + requests_needed)
            .checked_sub(REQUESTS_PER_HOUR + 1)
            .and_then(|release_index| active.nth(release_index))
            .map(|sent| *sent + WINDOW);
        self.blocked_until
            .filter(|retry_at| *retry_at > now)
            .into_iter()
            .chain(budget_retry)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_syncs_stay_within_thirty_requests_per_hour() {
        let start = Instant::now();
        let mut quota = RequestQuota::default();
        let mut requests = 0;
        for second in 0..3600 {
            let now = start + Duration::from_secs(second);
            if quota.begin_sync(now).is_ok() {
                for _ in 0..2 {
                    quota.record_request(now).unwrap();
                    requests += 1;
                }
            }
        }
        assert_eq!(requests, 30);
    }

    #[test]
    fn each_request_uses_the_sliding_budget() {
        let start = Instant::now();
        let mut quota = RequestQuota::default();
        for second in 0..30 {
            quota
                .record_request(start + Duration::from_secs(second))
                .unwrap();
        }
        assert_eq!(
            quota.record_request(start + Duration::from_secs(30)),
            Err(start + WINDOW)
        );
        quota.record_request(start + WINDOW).unwrap();
        assert_eq!(
            quota.record_request(start + WINDOW),
            Err(start + WINDOW + Duration::from_secs(1))
        );
    }

    #[test]
    fn quota_errors_block_all_requests_for_an_hour() {
        let start = Instant::now();
        let mut quota = RequestQuota::default();
        quota.begin_sync(start).unwrap();
        quota.record_request(start).unwrap();
        quota.block_after_quota_error(start);
        assert_eq!(quota.begin_sync(start + SYNC_INTERVAL), Err(start + WINDOW));
        assert_eq!(
            quota.record_request(start + SYNC_INTERVAL),
            Err(start + WINDOW)
        );
        quota.begin_sync(start + WINDOW).unwrap();
        quota.record_request(start + WINDOW).unwrap();
    }

    #[test]
    fn failed_and_partial_syncs_keep_their_cooldown() {
        let start = Instant::now();
        let mut quota = RequestQuota::default();
        quota.begin_sync(start).unwrap();
        quota.record_request(start).unwrap();
        assert_eq!(
            quota.begin_sync(start + Duration::from_secs(60)),
            Err(start + SYNC_INTERVAL)
        );
        quota.begin_sync(start + SYNC_INTERVAL).unwrap();
    }

    #[test]
    fn sync_waits_until_both_requests_fit_in_the_budget() {
        let start = Instant::now();
        let mut quota = RequestQuota::default();
        for second in 0..30 {
            quota
                .record_request(start + Duration::from_secs(second))
                .unwrap();
        }
        assert_eq!(
            quota.begin_sync(start + WINDOW),
            Err(start + WINDOW + Duration::from_secs(1))
        );
        let now = start + WINDOW + Duration::from_secs(1);
        quota.begin_sync(now).unwrap();
        quota.record_request(now).unwrap();
        quota.record_request(now).unwrap();
    }
}
