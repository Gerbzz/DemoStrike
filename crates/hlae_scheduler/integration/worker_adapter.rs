//! Example adapter to copy into the existing DemoStrike worker after review.
//! The worker supplies samples from its own offline demo clock and decides how
//! to consume the returned strings. This file performs no command execution.

use demostrike_hlae_scheduler::Scheduler;

pub struct WorkerSchedule {
    scheduler: Scheduler,
}

impl WorkerSchedule {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
        }
    }

    pub fn on_frame(&mut self, tick: Option<f32>, demo_time: Option<f32>) -> Vec<String> {
        self.scheduler.collect_frame(tick, demo_time)
    }

    pub fn on_level_init(&mut self) {
        self.scheduler.reset_level();
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.scheduler.set_enabled(enabled);
    }
    pub fn add_tick_command(&mut self, tick: f64, command: impl Into<String>) {
        self.scheduler.add_at_tick(command, tick);
    }
    pub fn add_time_command(&mut self, time: f64, command: impl Into<String>) {
        self.scheduler.add_at_time(command, time);
    }
}
