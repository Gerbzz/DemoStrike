//! Pure Rust scheduler port of advancedfx `CommandSystem` at commit
//! `7567c24ec2501a7b07cda6213ece4540e57b5039`.
//! Copyright (c) 2021 advancedfx.org. See `LICENSE-ADVANCEDFX`.
//! This crate returns inert command strings; it does not execute them.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Tick,
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub low: f64,
    pub high: f64,
    /// Controls inclusion of this interval's high edge.
    pub epsilon: bool,
}

impl Interval {
    pub fn new(low: f64, high: f64, epsilon: bool) -> Self {
        Self { low, high, epsilon }
    }

    fn overlaps(self, query: Self) -> bool {
        (if query.epsilon {
            self.low <= query.high
        } else {
            self.low < query.high
        }) && (if self.epsilon {
            query.low <= self.high
        } else {
            query.low < self.high
        })
    }
}

struct Entry {
    interval: Interval,
    command: String,
}

struct Node {
    interval: Interval,
    entry: usize,
    max: f64,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Default)]
struct Timeline {
    entries: Vec<Entry>,
    nodes: Vec<Node>,
    root: Option<usize>,
}

impl Timeline {
    fn ordered_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.entries.len()).collect();
        // C++ multimap compares Low then High and preserves insertion order
        // among equivalent keys. Epsilon is absent from its ordering.
        indices.sort_by(|&a, &b| {
            let a = self.entries[a].interval;
            let b = self.entries[b].interval;
            a.low
                .partial_cmp(&b.low)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.high.partial_cmp(&b.high).unwrap_or(Ordering::Equal))
        });
        indices
    }

    fn insert_node(&mut self, root: Option<usize>, interval: Interval, entry: usize) -> usize {
        let Some(root) = root else {
            let index = self.nodes.len();
            self.nodes.push(Node {
                interval,
                entry,
                max: interval.high,
                left: None,
                right: None,
            });
            return index;
        };
        if interval.low < self.nodes[root].interval.low {
            let child = self.insert_node(self.nodes[root].left, interval, entry);
            self.nodes[root].left = Some(child);
        } else {
            let child = self.insert_node(self.nodes[root].right, interval, entry);
            self.nodes[root].right = Some(child);
        }
        if self.nodes[root].max < interval.high {
            self.nodes[root].max = interval.high;
        }
        root
    }

    fn add(&mut self, interval: Interval, command: String) {
        let entry = self.entries.len();
        self.entries.push(Entry { interval, command });
        // Mirrors upstream insertion even when a previous edit invalidated the tree.
        self.root = Some(self.insert_node(self.root, interval, entry));
    }

    fn invalidate(&mut self) {
        self.root = None;
        self.nodes.clear();
    }

    fn ensure_tree(&mut self) {
        if self.root.is_some() {
            return;
        }
        for entry in self.ordered_indices() {
            let interval = self.entries[entry].interval;
            self.root = Some(self.insert_node(self.root, interval, entry));
        }
    }

    fn collect_node(&self, node: Option<usize>, query: Interval, out: &mut Vec<String>) {
        let Some(index) = node else {
            return;
        };
        let current = &self.nodes[index];
        if current.interval.overlaps(query) {
            out.push(self.entries[current.entry].command.clone());
        }
        // Pinned C++ `OverlapExecute` follows only one branch, which can miss
        // qualifying commands in the other subtree. Preserve that behavior.
        if let Some(left) = current.left {
            if self.nodes[left].max >= query.low {
                self.collect_node(Some(left), query, out);
                return;
            }
        }
        self.collect_node(current.right, query, out);
    }

    fn collect(&mut self, previous: f64, current: f64, out: &mut Vec<String>) {
        if self.entries.is_empty() {
            return;
        }
        self.ensure_tree();
        self.collect_node(self.root, Interval::new(previous, current, false), out);
    }

    fn remove(&mut self, ordered_index: usize) -> bool {
        let Some(entry) = self.ordered_indices().get(ordered_index).copied() else {
            return false;
        };
        self.entries.remove(entry);
        self.invalidate();
        true
    }

    fn edit_start(&mut self, start: f64) {
        if self.entries.is_empty() {
            return;
        }
        // The pinned C++ multimap is traversed in pre-shift key order and
        // reinserts in that order. Distinct keys can round to an equal key
        // after shifting, so insertion order alone is no longer sufficient.
        self.entries.sort_by(|a, b| {
            a.interval
                .low
                .partial_cmp(&b.interval.low)
                .unwrap_or(Ordering::Equal)
                .then_with(|| {
                    a.interval
                        .high
                        .partial_cmp(&b.interval.high)
                        .unwrap_or(Ordering::Equal)
                })
        });
        let offset = start - self.entries[0].interval.low;
        for entry in &mut self.entries {
            entry.interval.low += offset;
            entry.interval.high += offset;
        }
        self.invalidate();
    }
}

/// A clock sample is `f32` because the pinned C++ getters return `float`.
pub struct Scheduler {
    enabled: bool,
    last_tick: f64,
    last_time: f64,
    tick: Timeline,
    time: Timeline,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            enabled: true,
            last_tick: -1.0,
            last_time: -1.0,
            tick: Timeline::default(),
            time: Timeline::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn last_tick(&self) -> f64 {
        self.last_tick
    }
    pub fn last_time(&self) -> f64 {
        self.last_time
    }

    /// Mirrors `OnLevelInitPreEntity`; scheduled commands remain installed.
    pub fn reset_level(&mut self) {
        self.last_tick = -1.0;
        self.last_time = -1.0;
    }

    /// Clears commands without changing the previous clock samples.
    pub fn clear(&mut self) {
        self.tick = Timeline::default();
        self.time = Timeline::default();
    }

    pub fn add(&mut self, domain: Domain, interval: Interval, command: impl Into<String>) {
        match domain {
            Domain::Tick => self.tick.add(interval, command.into()),
            Domain::Time => self.time.add(interval, command.into()),
        }
    }

    pub fn add_at_tick(&mut self, command: impl Into<String>, tick: f64) {
        self.add(Domain::Tick, Interval::new(tick, tick, true), command);
    }
    pub fn add_at_time(&mut self, command: impl Into<String>, time: f64) {
        self.add(Domain::Time, Interval::new(time, time, true), command);
    }
    pub fn add_tick(&mut self, command: impl Into<String>) {
        self.add_at_tick(command, self.last_tick);
    }
    pub fn add_time(&mut self, command: impl Into<String>) {
        self.add_at_time(command, self.last_time);
    }
    pub fn edit_start_tick(&mut self, start: f64) {
        self.tick.edit_start(start);
    }
    pub fn edit_start_time(&mut self, start: f64) {
        self.time.edit_start(start);
    }

    /// Sorted tick entries come before sorted time entries, as in upstream.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.tick.entries.len() {
            self.tick.remove(index)
        } else {
            self.time.remove(index - self.tick.entries.len())
        }
    }

    /// Returns an inert batch; tick matches precede time matches. `None`
    /// means that domain's clock hook is unavailable for this frame.
    pub fn collect_frame(&mut self, tick: Option<f32>, time: Option<f32>) -> Vec<String> {
        let mut commands = Vec::new();
        if let Some(tick) = tick.map(f64::from) {
            if tick != 0.0 {
                if self.enabled && self.last_tick != tick {
                    self.tick.collect(self.last_tick, tick, &mut commands);
                }
                self.last_tick = tick;
            }
        }
        if let Some(time) = time.map(f64::from) {
            if self.enabled && self.last_time != time {
                self.time.collect(self.last_time, time, &mut commands);
            }
            self.last_time = time;
        }
        commands
    }

    /// Callback mutations affect later frames, never the current batch.
    /// The callback is caller-owned; this crate performs no execution.
    pub fn dispatch_frame(
        &mut self,
        tick: Option<f32>,
        time: Option<f32>,
        mut callback: impl FnMut(&str, &mut Self),
    ) {
        for command in self.collect_frame(tick, time) {
            callback(&command, self);
        }
    }
}
