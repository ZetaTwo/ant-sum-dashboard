//! Aggregates device updates into two nested periods of activity:
//!
//! - A **set** is one bout of group effort. It ends when activity drops to a
//!   fraction of the set's own recent average rather than to zero, since
//!   rests can be short and some machines keep reporting a trickle of
//!   movement while nobody is using them. A finished set stays on screen
//!   until the next one starts.
//! - A **session** spans consecutive sets and ends after
//!   `Config::session_timeout` without a new set, which resets the display.
//!
//! Both are presentation-layer only: they're baselines snapshotted from the
//! per-device totals, which themselves are never reset.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};

use crate::types::{DeviceRow, DeviceUpdate, SessionView, SetView, WsMessage};

/// Set detection windows, in ticks of the state actor's 1s timer.
///
/// The current activity level is the mean over the last `FAST_WINDOW`
/// ticks: distance only moves in 1 m steps and arrives roughly once a
/// second per device, so single ticks are too noisy to act on.
const FAST_WINDOW: usize = 5;
/// The set's reference level is the mean over up to `REFERENCE_WINDOW`
/// ticks before the fast window, so a drop-off doesn't drag down the level
/// it's being compared against.
const REFERENCE_WINDOW: usize = 30;
/// A set can't end before its reference level has this many ticks behind it.
const MIN_REFERENCE_TICKS: usize = 5;
/// Consecutive ticks a start/end condition must hold before acting on it.
const START_HOLD: usize = 2;
const END_HOLD: usize = 3;
const HISTORY_LEN: usize = REFERENCE_WINDOW + FAST_WINDOW + END_HOLD;
/// Devices silent for longer than this don't count towards the
/// per-device normalization. Matches the frontend's stale-row threshold.
const CONNECTED_WITHIN: Duration = Duration::from_secs(5);

type Totals = HashMap<u16, f64>;

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub session_timeout: Duration,
    /// A set ends once activity drops below this fraction of its reference
    /// level.
    pub set_end_fraction: f64,
    /// A new set starts once activity exceeds this fraction of the previous
    /// set's reference level (and `set_min_speed_mps`). Higher than
    /// `set_end_fraction`, so trickle around the end threshold can't
    /// immediately restart a set.
    pub set_start_fraction: f64,
    /// Absolute floor for starting a set, as mean speed per connected
    /// device. The only start criterion before the session's first set.
    pub set_min_speed_mps: f64,
}

struct DeviceState {
    device_type: crate::types::EquipmentType,
    /// Monotonic for the lifetime of the process. Never reset.
    total_distance_m: f64,
    last_seen: Instant,
    last_seen_wall: SystemTime,
}

/// Per-device totals at one tick, plus the activity level since the
/// previous tick.
struct Sample {
    at: Instant,
    wall: SystemTime,
    totals: Totals,
    /// Mean speed per connected device, in m/s. Normalizing by device count
    /// makes the thresholds mean the same with 2 machines as with 10.
    rate: f64,
}

struct SetEnd {
    at: Instant,
    wall: SystemTime,
    totals: Totals,
}

struct Set {
    baseline: Totals,
    started_at: SystemTime,
    /// Samples taken since the set started.
    ticks: usize,
    /// Updated while the set is running; after it ends, sets the bar for
    /// starting the next one.
    reference: f64,
    ended: Option<SetEnd>,
}

struct Session {
    baseline: Totals,
    started_at: SystemTime,
}

pub struct AppState {
    config: Config,
    devices: HashMap<u16, DeviceState>,
    history: VecDeque<Sample>,
    session: Option<Session>,
    set: Option<Set>,
    /// Consecutive ticks the pending start or end condition has held.
    hold: usize,
}

impl AppState {
    fn new(config: Config) -> Self {
        Self {
            config,
            devices: HashMap::new(),
            history: VecDeque::with_capacity(HISTORY_LEN + 1),
            session: None,
            set: None,
            hold: 0,
        }
    }

    fn apply_update(&mut self, update: DeviceUpdate) {
        let entry = self
            .devices
            .entry(update.device_id)
            .or_insert_with(|| DeviceState {
                device_type: update.device_type,
                total_distance_m: 0.0,
                last_seen: update.timestamp,
                last_seen_wall: SystemTime::now(),
            });
        entry.device_type = update.device_type;
        entry.total_distance_m = update.distance_m;
        entry.last_seen = update.timestamp;
        entry.last_seen_wall = SystemTime::now();
    }

    fn tick(&mut self, now: Instant, wall: SystemTime) {
        self.record_sample(now, wall);
        if self.set.as_ref().is_some_and(|set| set.ended.is_none()) {
            self.check_set_end();
        } else {
            self.check_session_end(now);
            self.check_set_start();
        }
    }

    fn totals(&self) -> Totals {
        self.devices
            .iter()
            .map(|(id, d)| (*id, d.total_distance_m))
            .collect()
    }

    fn record_sample(&mut self, now: Instant, wall: SystemTime) {
        let totals = self.totals();
        // Real ANT+ devices keep broadcasting their frozen distance while
        // idle, so activity is measured as distance gained, not messages
        // received.
        let connected = self
            .devices
            .values()
            .filter(|d| now.duration_since(d.last_seen) < CONNECTED_WITHIN)
            .count();
        let rate = match self.history.back() {
            Some(prev) if connected > 0 => {
                let gained: f64 = totals
                    .iter()
                    .map(|(id, total)| (total - prev.totals.get(id).unwrap_or(total)).max(0.0))
                    .sum();
                gained / now.duration_since(prev.at).as_secs_f64() / connected as f64
            }
            _ => 0.0,
        };

        self.history.push_back(Sample {
            at: now,
            wall,
            totals,
            rate,
        });
        if self.history.len() > HISTORY_LEN {
            self.history.pop_front();
        }
        if let Some(set) = &mut self.set
            && set.ended.is_none()
        {
            set.ticks += 1;
        }
    }

    fn fast_rate(&self) -> f64 {
        mean(self.history.iter().rev().take(FAST_WINDOW).map(|s| s.rate))
    }

    fn check_set_start(&mut self) {
        let previous_reference = self.set.as_ref().map_or(0.0, |set| set.reference);
        let threshold = self
            .config
            .set_min_speed_mps
            .max(self.config.set_start_fraction * previous_reference);

        if self.fast_rate() < threshold {
            self.hold = 0;
            return;
        }
        self.hold += 1;
        if self.hold < START_HOLD {
            return;
        }
        self.hold = 0;

        // Backdate the start to just before the first tick in the
        // triggering window that cleared the threshold, so the distance
        // covered while the condition was confirming counts towards the
        // set.
        let len = self.history.len();
        let window = (FAST_WINDOW + START_HOLD).min(len - 1);
        let first_active = (len - window..len)
            .find(|&i| self.history[i].rate >= threshold)
            .unwrap_or(len - window);
        let start = &self.history[first_active - 1];

        self.set = Some(Set {
            baseline: start.totals.clone(),
            started_at: start.wall,
            ticks: len - first_active,
            reference: 0.0,
            ended: None,
        });
        if self.session.is_none() {
            self.session = Some(Session {
                baseline: start.totals.clone(),
                started_at: start.wall,
            });
        }
    }

    fn check_set_end(&mut self) {
        let fast = self.fast_rate();
        let len = self.history.len();
        let Some(set) = &mut self.set else {
            return;
        };
        if set.ticks < FAST_WINDOW + MIN_REFERENCE_TICKS {
            return;
        }
        // Frozen while an end is pending, so the drop-off being confirmed
        // doesn't leak into the level it's compared against.
        if self.hold == 0 {
            let in_set = set.ticks.min(len) - FAST_WINDOW;
            set.reference = mean(
                self.history
                    .iter()
                    .rev()
                    .skip(FAST_WINDOW)
                    .take(in_set.min(REFERENCE_WINDOW))
                    .map(|s| s.rate),
            );
        }
        let threshold = self.config.set_end_fraction * set.reference;

        if fast >= threshold {
            self.hold = 0;
            return;
        }
        self.hold += 1;
        if self.hold < END_HOLD {
            return;
        }
        self.hold = 0;

        // Backdate the end to the last tick that was still above the
        // threshold, so the rest isn't counted as part of the set.
        let window = FAST_WINDOW + END_HOLD;
        let end = (len - window..len)
            .rev()
            .find(|&i| self.history[i].rate >= threshold)
            .unwrap_or(len - window - 1);
        let end = &self.history[end];
        set.ended = Some(SetEnd {
            at: end.at,
            wall: end.wall,
            totals: end.totals.clone(),
        });
    }

    fn check_session_end(&mut self, now: Instant) {
        let Some(Set {
            ended: Some(end), ..
        }) = &self.set
        else {
            return;
        };
        if now.duration_since(end.at) >= self.config.session_timeout {
            self.session = None;
            self.set = None;
        }
    }

    fn to_ws_message(&self) -> WsMessage {
        let live = self.totals();
        let set_totals = self
            .set
            .as_ref()
            .map(|set| set.ended.as_ref().map_or(&live, |end| &end.totals));

        let mut devices: Vec<DeviceRow> = self
            .devices
            .iter()
            .map(|(id, d)| DeviceRow {
                device_id: *id,
                device_type: d.device_type.as_str(),
                set_distance_m: self
                    .set
                    .as_ref()
                    .zip(set_totals)
                    .map_or(0.0, |(set, totals)| {
                        distance_since(&set.baseline, totals, *id)
                    }),
                session_distance_m: self
                    .session
                    .as_ref()
                    .map_or(0.0, |session| distance_since(&session.baseline, &live, *id)),
                last_seen_ms: to_epoch_ms(d.last_seen_wall),
            })
            .collect();
        devices.sort_by_key(|d| d.device_id);

        WsMessage::State {
            set: self.set.as_ref().map(|set| SetView {
                distance_m: devices.iter().map(|d| d.set_distance_m).sum(),
                started_at_ms: to_epoch_ms(set.started_at),
                ended_at_ms: set.ended.as_ref().map(|end| to_epoch_ms(end.wall)),
            }),
            session: self.session.as_ref().map(|session| SessionView {
                distance_m: devices.iter().map(|d| d.session_distance_m).sum(),
                started_at_ms: to_epoch_ms(session.started_at),
            }),
            devices,
        }
    }
}

/// A device missing from `baseline` joined after it was taken, when its
/// total was still 0; one missing from `totals` joined after a set ended.
fn distance_since(baseline: &Totals, totals: &Totals, id: u16) -> f64 {
    totals.get(&id).map_or(0.0, |total| {
        (total - baseline.get(&id).copied().unwrap_or(0.0)).max(0.0)
    })
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0), |(sum, count), v| (sum + v, count + 1));
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn to_epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Owns `AppState` exclusively (actor pattern, no shared `Mutex`): consumes
/// `DeviceUpdate`s from `rx`, runs set/session detection on a 1s timer, and
/// publishes `WsMessage::State` on `tx` after every update and every tick.
/// The per-tick publish also serves as a heartbeat that lets clients detect
/// a dead connection.
pub async fn run(
    mut rx: mpsc::Receiver<DeviceUpdate>,
    tx: watch::Sender<WsMessage>,
    config: Config,
) {
    let mut state = AppState::new(config);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            maybe_update = rx.recv() => {
                match maybe_update {
                    Some(update) => state.apply_update(update),
                    None => break, // all senders dropped
                }
            }
            _ = ticker.tick() => state.tick(Instant::now(), SystemTime::now()),
        }
        let _ = tx.send(state.to_ws_message());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EquipmentType;

    const WORK: f64 = 3.0;
    const TRICKLE: f64 = 0.2;

    fn config() -> Config {
        Config {
            session_timeout: Duration::from_secs(60),
            set_end_fraction: 0.25,
            set_start_fraction: 0.4,
            set_min_speed_mps: 0.5,
        }
    }

    /// Drives `AppState` with simulated time: each second every device
    /// reports its distance advanced by its given speed, then the state
    /// ticks.
    struct Harness {
        state: AppState,
        now: Instant,
        wall: SystemTime,
        distances: HashMap<u16, f64>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                state: AppState::new(config()),
                now: Instant::now(),
                wall: UNIX_EPOCH,
                distances: HashMap::new(),
            }
        }

        fn run(&mut self, secs: u64, speeds: &[(u16, f64)]) {
            for _ in 0..secs {
                self.now += Duration::from_secs(1);
                self.wall += Duration::from_secs(1);
                for &(device_id, speed) in speeds {
                    let distance = self.distances.entry(device_id).or_default();
                    *distance += speed;
                    self.state.apply_update(DeviceUpdate {
                        device_id,
                        device_type: EquipmentType::Rower,
                        distance_m: *distance,
                        speed_mps: None,
                        heart_rate: None,
                        timestamp: self.now,
                    });
                }
                self.state.tick(self.now, self.wall);
            }
        }

        fn view(&self) -> (Option<SetView>, Option<SessionView>) {
            let WsMessage::State { set, session, .. } = self.state.to_ws_message();
            (set, session)
        }

        fn elapsed_secs(&self, epoch_ms: u64) -> u64 {
            to_epoch_ms(self.wall) / 1000 - epoch_ms / 1000
        }
    }

    fn all(devices: u16, speed: f64) -> Vec<(u16, f64)> {
        (1..=devices).map(|id| (id, speed)).collect()
    }

    #[test]
    fn trickle_alone_never_starts_a_set() {
        let mut h = Harness::new();
        h.run(120, &all(3, TRICKLE));
        let (set, session) = h.view();
        assert!(set.is_none());
        assert!(session.is_none());
    }

    #[test]
    fn set_start_is_backdated_to_include_the_confirming_ticks() {
        let mut h = Harness::new();
        h.run(10, &all(2, 0.0));
        h.run(20, &all(2, WORK));
        let (set, session) = h.view();
        let set = set.expect("set running");
        assert_eq!(set.ended_at_ms, None);
        assert_eq!(set.distance_m, 2.0 * 20.0 * WORK);
        assert_eq!(h.elapsed_secs(set.started_at_ms), 20);
        assert_eq!(session.expect("session running").distance_m, set.distance_m);
    }

    #[test]
    fn set_ends_on_trickle_and_keeps_its_numbers_through_the_rest() {
        let mut h = Harness::new();
        h.run(5, &all(3, 0.0));
        h.run(30, &all(3, WORK));
        h.run(20, &all(3, TRICKLE));
        let (set, session) = h.view();
        let set = set.expect("finished set still shown");
        // The end is backdated to the last working tick; the trickle during
        // the rest doesn't count towards the set...
        assert_eq!(set.distance_m, 3.0 * 30.0 * WORK);
        assert_eq!(h.elapsed_secs(set.ended_at_ms.expect("set ended")), 20);
        // ...but does towards the session.
        let session = session.expect("session continues");
        assert!((session.distance_m - 3.0 * (30.0 * WORK + 20.0 * TRICKLE)).abs() < 1e-9);
    }

    #[test]
    fn next_set_replaces_the_previous_one_and_session_keeps_counting() {
        let mut h = Harness::new();
        h.run(5, &all(2, 0.0));
        h.run(30, &all(2, WORK));
        h.run(20, &all(2, TRICKLE));
        h.run(15, &all(2, WORK));
        let (set, session) = h.view();
        let set = set.expect("second set running");
        assert_eq!(set.ended_at_ms, None);
        assert!((set.distance_m - 2.0 * 15.0 * WORK).abs() < 1e-9);
        let session = session.expect("session continues");
        assert_eq!(h.elapsed_secs(session.started_at_ms), 65);
    }

    #[test]
    fn one_of_two_people_stopping_does_not_end_the_set() {
        let mut h = Harness::new();
        h.run(30, &all(2, WORK));
        h.run(30, &[(1, WORK), (2, TRICKLE)]);
        let (set, _) = h.view();
        assert_eq!(set.expect("set running").ended_at_ms, None);
    }

    #[test]
    fn detection_is_the_same_for_two_and_ten_devices() {
        let mut outcomes = Vec::new();
        for devices in [2, 10] {
            let mut h = Harness::new();
            h.run(10, &all(devices, 0.0));
            h.run(40, &all(devices, WORK));
            h.run(20, &all(devices, TRICKLE));
            h.run(25, &all(devices, WORK));
            let (set, session) = h.view();
            let (set, session) = (set.unwrap(), session.unwrap());
            outcomes.push((
                h.elapsed_secs(set.started_at_ms),
                h.elapsed_secs(session.started_at_ms),
                set.distance_m / devices as f64,
            ));
        }
        assert_eq!(outcomes[0], outcomes[1]);
    }

    #[test]
    fn session_ends_after_timeout_without_a_new_set() {
        let mut h = Harness::new();
        h.run(30, &all(2, WORK));
        h.run(59, &all(2, TRICKLE));
        assert!(h.view().1.is_some());
        h.run(1, &all(2, TRICKLE));
        let (set, session) = h.view();
        assert!(set.is_none());
        assert!(session.is_none());
    }

    #[test]
    fn new_session_after_timeout_starts_from_zero() {
        let mut h = Harness::new();
        h.run(30, &all(2, WORK));
        h.run(70, &all(2, TRICKLE));
        h.run(20, &all(2, WORK));
        let (set, session) = h.view();
        let (set, session) = (set.unwrap(), session.unwrap());
        assert!((set.distance_m - 2.0 * 20.0 * WORK).abs() < 1e-9);
        assert_eq!(session.distance_m, set.distance_m);
    }
}
