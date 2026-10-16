//! Independent task timers with explicit timestamps.
//!
//! Supply timestamps from one monotonic origin. Do not use calendar timestamps.
//! The caller supplies time so tests do not need a system clock or delays.

use std::time::Duration;

#[derive(Clone, Copy)]
enum TimerState {
    Stopped(Duration),
    Running {
        elapsed: Duration,
        started_at: Duration,
    },
}

/// A named task with an independent timer.
pub struct Task {
    description: String,
    timer: TimerState,
}

impl Task {
    /// Creates a stopped task with zero elapsed time.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            timer: TimerState::Stopped(Duration::ZERO),
        }
    }

    /// Returns the task description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns true while the task timer runs.
    pub fn is_running(&self) -> bool {
        matches!(self.timer, TimerState::Running { .. })
    }

    /// Starts or resumes the timer at the supplied timestamp.
    /// A repeated start leaves the active interval unchanged.
    pub fn start(&mut self, now: Duration) {
        if let TimerState::Stopped(elapsed) = self.timer {
            self.timer = TimerState::Running {
                elapsed,
                started_at: now,
            };
        }
    }

    /// Stops the timer and preserves its accumulated time.
    /// A repeated stop leaves the accumulated time unchanged.
    ///
    /// # Panics
    ///
    /// Panics if a running timer receives a timestamp before its current start.
    pub fn stop(&mut self, now: Duration) {
        self.timer = TimerState::Stopped(self.elapsed_at(now));
    }

    /// Returns accumulated time, including the current running interval.
    /// Time between a stop and the next start does not count.
    ///
    /// # Panics
    ///
    /// Panics if a running timer receives a timestamp before its current start.
    pub fn elapsed_at(&self, now: Duration) -> Duration {
        match self.timer {
            TimerState::Stopped(elapsed) => elapsed,
            TimerState::Running {
                elapsed,
                started_at,
            } => elapsed + (now - started_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_task_is_stopped_with_zero_elapsed_time() {
        let task = Task::new("Write tests");

        assert_eq!(task.description(), "Write tests");
        assert!(!task.is_running());
        assert_eq!(task.elapsed_at(Duration::from_secs(100)), Duration::ZERO);
    }

    #[test]
    fn running_task_reports_elapsed_time() {
        let mut task = Task::new("Write tests");

        task.start(Duration::from_secs(100));

        assert!(task.is_running());
        assert_eq!(
            task.elapsed_at(Duration::from_secs(130)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn stopped_task_keeps_its_elapsed_time() {
        let mut task = Task::new("Write tests");
        task.start(Duration::from_secs(100));

        task.stop(Duration::from_secs(140));

        assert!(!task.is_running());
        assert_eq!(
            task.elapsed_at(Duration::from_secs(200)),
            Duration::from_secs(40)
        );
    }

    #[test]
    fn resumed_task_accumulates_time_without_counting_the_pause() {
        let mut task = Task::new("Write tests");
        task.start(Duration::from_secs(100));
        task.stop(Duration::from_secs(140));

        task.start(Duration::from_secs(210));
        task.stop(Duration::from_secs(230));

        assert_eq!(
            task.elapsed_at(Duration::from_secs(300)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn overlapping_tasks_run_independently() {
        let mut first = Task::new("Write tests");
        let mut second = Task::new("Review a change");
        first.start(Duration::from_secs(100));
        second.start(Duration::from_secs(110));

        first.stop(Duration::from_secs(140));

        assert!(!first.is_running());
        assert!(second.is_running());
        assert_eq!(
            first.elapsed_at(Duration::from_secs(150)),
            Duration::from_secs(40)
        );
        assert_eq!(
            second.elapsed_at(Duration::from_secs(150)),
            Duration::from_secs(40)
        );

        second.stop(Duration::from_secs(160));

        assert!(!second.is_running());
        assert_eq!(
            second.elapsed_at(Duration::from_secs(200)),
            Duration::from_secs(50)
        );
        assert_eq!(
            first.elapsed_at(Duration::from_secs(200)),
            Duration::from_secs(40)
        );
    }

    #[test]
    fn starting_a_running_task_keeps_the_original_start_time() {
        let mut task = Task::new("Write tests");
        task.start(Duration::from_secs(100));

        task.start(Duration::from_secs(120));

        assert!(task.is_running());
        assert_eq!(
            task.elapsed_at(Duration::from_secs(130)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn stopping_a_stopped_task_keeps_accumulated_time() {
        let mut task = Task::new("Write tests");
        task.stop(Duration::from_secs(90));
        assert!(!task.is_running());
        assert_eq!(task.elapsed_at(Duration::from_secs(100)), Duration::ZERO);
        task.start(Duration::from_secs(100));
        task.stop(Duration::from_secs(140));

        task.stop(Duration::from_secs(200));

        assert!(!task.is_running());
        assert_eq!(
            task.elapsed_at(Duration::from_secs(250)),
            Duration::from_secs(40)
        );

        task.start(Duration::from_secs(300));
        task.stop(Duration::from_secs(320));

        assert_eq!(
            task.elapsed_at(Duration::from_secs(400)),
            Duration::from_secs(60)
        );
    }
}
