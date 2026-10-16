use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Free Toggl plans allow 30 requests per hour to the `/me` endpoints.
const REQUESTS_PER_HOUR: usize = 30;
const WINDOW: Duration = Duration::from_secs(60 * 60);

/// Counts the requests of one import in a sliding hour, so that an import
/// stops before Toggl rejects it.
#[derive(Default)]
pub struct RequestQuota {
    requests: VecDeque<Instant>,
}

impl RequestQuota {
    /// Records a request, or returns when the next request may run.
    pub fn record_request(&mut self, now: Instant) -> Result<(), Instant> {
        while self
            .requests
            .front()
            .is_some_and(|sent| *sent + WINDOW <= now)
        {
            self.requests.pop_front();
        }
        if self.requests.len() >= REQUESTS_PER_HOUR {
            return Err(self.requests[0] + WINDOW);
        }
        self.requests.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
