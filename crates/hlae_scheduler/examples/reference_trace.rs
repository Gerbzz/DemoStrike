use demostrike_hlae_scheduler::{Domain, Interval, Scheduler};

fn emit(label: &str, commands: Vec<String>) {
    println!("{}|{}", label, commands.join(","));
}

fn main() {
    {
        let mut s = Scheduler::new();
        s.add_at_tick("one", 1.0);
        s.add_at_time("time", 0.0);
        emit("initial", s.collect_frame(Some(1.0), Some(0.0)));
        emit("tick_zero", s.collect_frame(Some(0.0), Some(0.0)));
        emit("forward", s.collect_frame(Some(2.0), Some(1.0)));
        emit("repeat", s.collect_frame(Some(2.0), Some(1.0)));
        s.reset_level();
        emit("reset", s.collect_frame(Some(2.0), Some(1.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("first", 3.0);
        s.add_at_tick("second", 3.0);
        s.collect_frame(Some(3.0), Some(0.0));
        emit("same_tick", s.collect_frame(Some(4.0), Some(0.0)));
        emit("backward", s.collect_frame(Some(2.0), Some(0.0)));
        emit("seek_forward", s.collect_frame(Some(4.0), Some(0.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add(Domain::Tick, Interval::new(2.0, 4.0, true), "closed");
        s.add(Domain::Tick, Interval::new(2.0, 4.0, false), "open");
        s.collect_frame(Some(2.0), Some(0.0));
        emit("range_inside", s.collect_frame(Some(4.0), Some(0.0)));
        emit("range_end", s.collect_frame(Some(5.0), Some(0.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("root", 5.0);
        s.add_at_tick("left", 3.0);
        s.add_at_tick("right", 6.0);
        s.collect_frame(Some(2.0), Some(0.0));
        emit("one_branch", s.collect_frame(Some(7.0), Some(0.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("tick", 1.0);
        s.add_at_time("time", 1.0);
        s.collect_frame(Some(1.0), Some(1.0));
        emit("tick_before_time", s.collect_frame(Some(2.0), Some(2.0)));
        s.set_enabled(false);
        emit("disabled", s.collect_frame(Some(3.0), Some(3.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("older", 1.0);
        s.add_at_tick("removed", 2.0);
        s.remove(1);
        s.add_at_tick("new", 3.0);
        s.collect_frame(Some(0.5), Some(0.0));
        emit("partial_tree", s.collect_frame(Some(4.0), Some(0.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("a", 10.0);
        s.add_at_tick("b", 12.0);
        s.edit_start_tick(20.0);
        s.reset_level();
        s.collect_frame(Some(20.0), Some(0.0));
        emit("edit_start", s.collect_frame(Some(21.0), Some(0.0)));
        s.collect_frame(Some(22.0), Some(0.0));
        emit("edit_second", s.collect_frame(Some(23.0), Some(0.0)));
    }
    {
        let mut s = Scheduler::new();
        s.add_at_tick("first", 3.0);
        s.add_at_tick("second", 3.0);
        s.collect_frame(Some(3.0), Some(0.0));
        let mut delivered = Vec::new();
        s.dispatch_frame(Some(4.0), Some(0.0), |command, scheduler| {
            delivered.push(command.to_owned());
            scheduler.clear();
            scheduler.set_enabled(false);
        });
        emit("callback_mutation", delivered);
    }
}
