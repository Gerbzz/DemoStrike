#[path = "../integration/worker_adapter.rs"]
mod worker_adapter;

use worker_adapter::WorkerSchedule;

#[test]
fn worker_adapter_returns_inert_commands_from_offline_clock() {
    let mut worker = WorkerSchedule::new();
    worker.add_tick_command(3.0, "marker");
    assert!(worker.on_frame(Some(3.0), None).is_empty());
    assert_eq!(worker.on_frame(Some(4.0), None), ["marker"]);
    worker.on_level_init();
    assert_eq!(worker.on_frame(Some(4.0), None), ["marker"]);
    worker.add_time_command(2.0, "time_marker");
    assert!(worker.on_frame(None, Some(2.0)).is_empty());
    worker.set_enabled(false);
    assert!(worker.on_frame(None, Some(3.0)).is_empty());
    worker.set_enabled(true);
    worker.on_level_init();
    assert_eq!(worker.on_frame(None, Some(3.0)), ["time_marker"]);
}
