use demostrike_hlae_scheduler::{Domain, Interval, Scheduler};

#[test]
fn tick_one_shot_fires_on_departure_and_skipped_tick() {
    let mut s = Scheduler::new();
    s.add_at_tick("at_two", 2.0);
    s.add_at_tick("at_three", 3.0);
    assert_eq!(s.collect_frame(Some(2.0), None), Vec::<String>::new());
    assert_eq!(s.collect_frame(Some(5.0), None), ["at_two", "at_three"]);
    assert!(s.collect_frame(Some(5.0), None).is_empty());
}

#[test]
fn initial_tick_zero_does_not_replace_initial_minus_one_cursor() {
    let mut s = Scheduler::new();
    s.add_at_tick("zero", 0.0);
    assert!(s.collect_frame(Some(0.0), None).is_empty());
    assert_eq!(s.last_tick(), -1.0);
    assert_eq!(s.collect_frame(Some(1.0), None), ["zero"]);
}

#[test]
fn tick_zero_is_ignored_and_reset_rewinds_cursor() {
    let mut s = Scheduler::new();
    s.add_at_tick("one", 1.0);
    assert!(s.collect_frame(Some(1.0), None).is_empty());
    assert!(s.collect_frame(Some(0.0), None).is_empty());
    assert_eq!(s.last_tick(), 1.0);
    assert_eq!(s.collect_frame(Some(2.0), None), ["one"]);
    s.reset_level();
    assert_eq!(s.last_tick(), -1.0);
    assert!(s.collect_frame(Some(1.0), None).is_empty());
    assert_eq!(s.collect_frame(Some(2.0), None), ["one"]);
}

#[test]
fn backwards_seek_rearms_for_forward_traversal() {
    let mut s = Scheduler::new();
    s.add_at_tick("three", 3.0);
    assert!(s.collect_frame(Some(3.0), None).is_empty());
    assert_eq!(s.collect_frame(Some(4.0), None), ["three"]);
    assert!(s.collect_frame(Some(2.0), None).is_empty());
    assert_eq!(s.collect_frame(Some(4.0), None), ["three"]);
}

#[test]
fn time_zero_works_and_tick_batch_precedes_time_batch() {
    let mut s = Scheduler::new();
    s.add_at_tick("tick", 1.0);
    s.add_at_time("time_zero", 0.0);
    s.add_at_time("time_one", 1.0);
    assert_eq!(s.collect_frame(Some(1.0), Some(0.0)), Vec::<String>::new());
    assert_eq!(
        s.collect_frame(Some(2.0), Some(2.0)),
        ["tick", "time_zero", "time_one"]
    );
}

#[test]
fn same_tick_order_follows_insertion_order() {
    let mut s = Scheduler::new();
    s.add_at_tick("first", 3.0);
    s.add_at_tick("second", 3.0);
    s.add_at_tick("third", 3.0);
    s.collect_frame(Some(3.0), None);
    assert_eq!(
        s.collect_frame(Some(4.0), None),
        ["first", "second", "third"]
    );
}

#[test]
fn interval_endpoints_respect_epsilon_and_open_frame_upper_edge() {
    let mut s = Scheduler::new();
    s.add(Domain::Tick, Interval::new(2.0, 4.0, true), "closed");
    s.add(Domain::Tick, Interval::new(2.0, 4.0, false), "open");
    assert!(s.collect_frame(Some(2.0), None).is_empty());
    assert_eq!(s.collect_frame(Some(4.0), None), ["closed", "open"]);
    assert_eq!(s.collect_frame(Some(5.0), None), ["closed"]);
}

#[test]
fn disabled_frames_advance_cursor_without_catch_up() {
    let mut s = Scheduler::new();
    s.add_at_tick("three", 3.0);
    s.set_enabled(false);
    assert!(s.collect_frame(Some(4.0), None).is_empty());
    s.set_enabled(true);
    assert!(s.collect_frame(Some(5.0), None).is_empty());
    assert_eq!(s.last_tick(), 5.0);
}

#[test]
fn callback_mutations_do_not_change_collected_batch() {
    let mut s = Scheduler::new();
    s.add_at_tick("first", 3.0);
    s.add_at_tick("second", 3.0);
    s.collect_frame(Some(3.0), None);
    let mut delivered = Vec::new();
    s.dispatch_frame(Some(4.0), None, |command, scheduler| {
        delivered.push(command.to_owned());
        scheduler.clear();
        scheduler.set_enabled(false);
    });
    assert_eq!(delivered, ["first", "second"]);
}

#[test]
fn remove_uses_tick_then_time_index_order() {
    let mut s = Scheduler::new();
    s.add_at_time("time", 1.0);
    s.add_at_tick("tick", 1.0);
    assert!(s.remove(0));
    assert!(!s.remove(9));
    s.collect_frame(Some(1.0), Some(1.0));
    assert_eq!(s.collect_frame(Some(2.0), Some(2.0)), ["time"]);
}

#[test]
fn add_at_last_tick_uses_cursor_and_edit_start_shifts_all() {
    let mut s = Scheduler::new();
    s.collect_frame(Some(7.0), None);
    s.add_tick("current");
    assert_eq!(s.collect_frame(Some(8.0), None), ["current"]);
    s.clear();
    s.add_at_tick("a", 10.0);
    s.add_at_tick("b", 12.0);
    s.edit_start_tick(20.0);
    s.reset_level();
    s.collect_frame(Some(20.0), None);
    assert_eq!(s.collect_frame(Some(21.0), None), ["a"]);
    s.collect_frame(Some(22.0), None);
    assert_eq!(s.collect_frame(Some(23.0), None), ["b"]);
}

#[test]
fn edit_start_preserves_pre_shift_order_when_distinct_keys_round_equal() {
    let mut s = Scheduler::new();
    s.add_at_tick("B", 1.0000000000000002);
    s.add_at_tick("A", 1.0);
    s.edit_start_tick(1000.0);
    assert!(s.collect_frame(Some(1000.0), None).is_empty());
    assert_eq!(s.collect_frame(Some(1001.0), None), ["A", "B"]);
}

#[test]
fn pinned_tree_traversal_takes_only_left_branch_when_eligible() {
    let mut s = Scheduler::new();
    s.add_at_tick("root", 5.0);
    s.add_at_tick("left", 3.0);
    s.add_at_tick("right", 6.0);
    s.collect_frame(Some(2.0), None);
    assert_eq!(s.collect_frame(Some(7.0), None), ["root", "left"]);
}

#[test]
fn time_cursor_and_shift_follow_tick_rules_except_zero() {
    let mut s = Scheduler::new();
    s.add_at_time("first", 2.0);
    s.add_at_time("second", 4.0);
    s.edit_start_time(10.0);
    assert!(s.collect_frame(None, Some(10.0)).is_empty());
    assert_eq!(s.collect_frame(None, Some(11.0)), ["first"]);
    s.add_time("current");
    assert_eq!(s.collect_frame(None, Some(12.0)), ["current"]);
    assert_eq!(s.last_time(), 12.0);
}

#[test]
fn clock_samples_are_rounded_to_f32_before_comparison() {
    let mut s = Scheduler::new();
    s.add_at_tick("point", 16_777_217.0);
    assert!(s.collect_frame(Some(16_777_216.0), None).is_empty());
    assert!(s.collect_frame(Some(16_777_217.0), None).is_empty());
    assert_eq!(s.last_tick(), 16_777_216.0);
    assert_eq!(s.collect_frame(Some(16_777_218.0), None), ["point"]);
}

#[test]
fn add_after_tree_invalidation_preserves_upstream_partial_tree() {
    let mut s = Scheduler::new();
    s.add_at_tick("older", 1.0);
    s.add_at_tick("removed", 2.0);
    assert!(s.remove(1));
    s.add_at_tick("new", 3.0);
    s.collect_frame(Some(0.5), None);
    assert_eq!(s.collect_frame(Some(4.0), None), ["new"]);
}
