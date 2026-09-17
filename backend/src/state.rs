use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};

use crate::types::{DeviceRow, DeviceUpdate, WsMessage};

struct DeviceState {
    device_type: crate::types::EquipmentType,
    /// Monotonic for the lifetime of the process. Never reset.
    total_distance_m: f64,
    last_seen: Instant,
    last_seen_wall: SystemTime,
}

struct SessionState {
    /// Snapshot of each device's `total_distance_m` at the moment the
    /// current session started. Presentation-layer only: subtracting this
    /// from a device's live total is what makes the displayed number
    /// appear to reset to zero, without ever touching the real total.
    baseline: HashMap<u16, f64>,
    session_started_at: Instant,
    session_started_at_wall: SystemTime,
    /// Guards against re-snapshotting the baseline on every timer tick
    /// while idle; only trigger once per idle period.
    is_reset: bool,
}

pub struct AppState {
    devices: HashMap<u16, DeviceState>,
    session: SessionState,
    global_last_activity: Instant,
    inactivity_timeout: Duration,
}

impl AppState {
    fn new(inactivity_timeout: Duration) -> Self {
        let now = Instant::now();
        Self {
            devices: HashMap::new(),
            session: SessionState {
                baseline: HashMap::new(),
                session_started_at: now,
                session_started_at_wall: SystemTime::now(),
                is_reset: true,
            },
            global_last_activity: now,
            inactivity_timeout,
        }
    }

    fn apply_update(&mut self, update: DeviceUpdate) {
        // Real ANT+ devices keep broadcasting at a fixed rate even while
        // idle (observed directly: a stationary bike kept re-sending the
        // same frozen distance every ~1.25s indefinitely). So "a message
        // arrived" is not "activity" for inactivity-reset purposes - only
        // genuine forward progress is. A brand new device counts as
        // activity too (it just connected).
        let moved = match self.devices.get(&update.device_id) {
            Some(existing) => update.distance_m > existing.total_distance_m,
            None => true,
        };

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
        // last_seen updates on every broadcast regardless of movement -
        // it's "is this device still connected", a different concept from
        // "is anyone actively moving" (which drives the inactivity reset).
        entry.last_seen = update.timestamp;
        entry.last_seen_wall = SystemTime::now();

        if moved {
            self.global_last_activity = update.timestamp;
            self.session.is_reset = false;
        }
    }

    /// Returns true if a new session was started (baseline re-snapshotted).
    fn maybe_reset_session(&mut self, now: Instant) -> bool {
        if self.session.is_reset {
            return false;
        }
        if !should_reset(self.global_last_activity, now, self.inactivity_timeout) {
            return false;
        }
        self.session.baseline = self
            .devices
            .iter()
            .map(|(id, d)| (*id, d.total_distance_m))
            .collect();
        self.session.session_started_at = now;
        self.session.session_started_at_wall = SystemTime::now();
        self.session.is_reset = true;
        true
    }

    fn to_ws_message(&self) -> WsMessage {
        let mut total_distance_m = 0.0;
        let mut devices = Vec::with_capacity(self.devices.len());
        for (id, d) in &self.devices {
            let baseline = self.session.baseline.get(id).copied().unwrap_or(0.0);
            let displayed = (d.total_distance_m - baseline).max(0.0);
            total_distance_m += displayed;
            devices.push(DeviceRow {
                device_id: *id,
                device_type: d.device_type.as_str(),
                distance_m: displayed,
                last_seen_ms: to_epoch_ms(d.last_seen_wall),
            });
        }
        devices.sort_by_key(|d| d.device_id);
        // No session is actively running until the first device reports
        // after (re)start or after an inactivity reset - `is_reset` tracks
        // exactly that, so reuse it rather than a sentinel timestamp.
        let session_started_at_ms =
            (!self.session.is_reset).then(|| to_epoch_ms(self.session.session_started_at_wall));
        WsMessage::State {
            total_distance_m,
            session_started_at_ms,
            devices,
        }
    }
}

fn to_epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Pure boundary check, kept separate from `AppState` so it's testable
/// without needing to actually sleep in tests.
fn should_reset(last_activity: Instant, now: Instant, timeout: Duration) -> bool {
    now.duration_since(last_activity) >= timeout
}

/// Owns `AppState` exclusively (actor pattern, no shared `Mutex`): consumes
/// `DeviceUpdate`s from `rx`, runs the inactivity timer, and publishes
/// `WsMessage::State` on `tx` after every change.
pub async fn run(
    mut rx: mpsc::Receiver<DeviceUpdate>,
    tx: watch::Sender<WsMessage>,
    inactivity_timeout: Duration,
) {
    let mut state = AppState::new(inactivity_timeout);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            maybe_update = rx.recv() => {
                match maybe_update {
                    Some(update) => {
                        state.apply_update(update);
                        let _ = tx.send(state.to_ws_message());
                    }
                    None => break, // all senders dropped
                }
            }
            _ = ticker.tick() => {
                if state.maybe_reset_session(Instant::now()) {
                    let _ = tx.send(state.to_ws_message());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EquipmentType;

    fn update(device_id: u16, distance_m: f64, t: Instant) -> DeviceUpdate {
        DeviceUpdate {
            device_id,
            device_type: EquipmentType::Bike,
            distance_m,
            speed_mps: None,
            heart_rate: None,
            timestamp: t,
        }
    }

    #[test]
    fn should_reset_boundary_conditions() {
        let start = Instant::now();
        let timeout = Duration::from_secs(30);
        assert!(!should_reset(
            start,
            start + Duration::from_secs(29),
            timeout
        ));
        assert!(should_reset(
            start,
            start + Duration::from_secs(30),
            timeout
        ));
        assert!(should_reset(
            start,
            start + Duration::from_secs(31),
            timeout
        ));
    }

    #[test]
    fn reset_zeroes_displayed_without_touching_total() {
        let mut state = AppState::new(Duration::from_secs(30));
        let t0 = Instant::now();
        state.apply_update(update(1, 100.0, t0));

        let reset_happened = state.maybe_reset_session(t0 + Duration::from_secs(31));
        assert!(reset_happened);

        assert_eq!(state.devices.get(&1).unwrap().total_distance_m, 100.0);
        match state.to_ws_message() {
            WsMessage::State {
                total_distance_m,
                devices,
                ..
            } => {
                assert_eq!(total_distance_m, 0.0);
                assert_eq!(devices[0].distance_m, 0.0);
            }
        }
    }

    #[test]
    fn update_after_reset_shows_only_the_delta() {
        let mut state = AppState::new(Duration::from_secs(30));
        let t0 = Instant::now();
        state.apply_update(update(1, 100.0, t0));
        state.maybe_reset_session(t0 + Duration::from_secs(31));

        state.apply_update(update(1, 142.0, t0 + Duration::from_secs(32)));
        match state.to_ws_message() {
            WsMessage::State {
                total_distance_m,
                devices,
                ..
            } => {
                assert_eq!(total_distance_m, 42.0);
                assert_eq!(devices[0].distance_m, 42.0);
            }
        }
        assert_eq!(state.devices.get(&1).unwrap().total_distance_m, 142.0);
    }

    #[test]
    fn reset_does_not_retrigger_every_tick_while_idle() {
        let mut state = AppState::new(Duration::from_secs(30));
        let t0 = Instant::now();
        state.apply_update(update(1, 100.0, t0));

        assert!(state.maybe_reset_session(t0 + Duration::from_secs(31)));
        // Still idle a tick later: must not reset again (already reset).
        assert!(!state.maybe_reset_session(t0 + Duration::from_secs(32)));
    }

    #[test]
    fn repeated_broadcasts_with_unchanged_distance_do_not_block_reset() {
        // Regression test: real ANT+ devices keep broadcasting at a fixed
        // rate even while idle, re-sending the same frozen distance. That
        // must not look like "activity" and keep pushing the inactivity
        // timeout out indefinitely.
        let mut state = AppState::new(Duration::from_secs(30));
        let t0 = Instant::now();
        state.apply_update(update(1, 100.0, t0));
        state.apply_update(update(1, 100.0, t0 + Duration::from_secs(10)));
        state.apply_update(update(1, 100.0, t0 + Duration::from_secs(20)));

        // 31s after the last genuine movement (t0), not the last message.
        assert!(state.maybe_reset_session(t0 + Duration::from_secs(31)));
    }
}
