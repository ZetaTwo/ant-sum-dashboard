use std::time::{Duration, Instant};

use rand::Rng;
use tokio::sync::mpsc;

use crate::types::{DeviceUpdate, EquipmentType};

struct FakeDevice {
    device_id: u16,
    device_type: EquipmentType,
    distance_m: f64,
    speed_mps: f32,
    tick_interval: Duration,
}

/// Mirrors `ant_source::usb::run`'s signature so `main.rs` can pick either
/// source without the rest of the pipeline (state/ws) knowing the
/// difference.
pub async fn run(
    tx: mpsc::Sender<DeviceUpdate>,
    inactivity_timeout: Duration,
) -> anyhow::Result<()> {
    let devices = vec![
        FakeDevice {
            device_id: 1001,
            device_type: EquipmentType::Bike,
            distance_m: 0.0,
            speed_mps: 6.0,
            tick_interval: Duration::from_millis(1000),
        },
        FakeDevice {
            device_id: 1002,
            device_type: EquipmentType::Rower,
            distance_m: 0.0,
            speed_mps: 3.0,
            tick_interval: Duration::from_millis(1000),
        },
        FakeDevice {
            device_id: 1003,
            device_type: EquipmentType::NordicSkier,
            distance_m: 0.0,
            speed_mps: 4.0,
            tick_interval: Duration::from_millis(1000),
        },
    ];

    // Real ANT+ hardware keeps broadcasting on a fixed schedule even while
    // idle, just with frozen distance (see ant_source::usb / the
    // `apply_update` "moved" check in state.rs, added after exactly this
    // was found to silently defeat the inactivity reset). Cycle each
    // device between moving and idle-but-still-transmitting so `--simulate`
    // exercises the reset path the same way real hardware does.
    let active_duration = inactivity_timeout.mul_f64(2.0);
    let cycle_duration = active_duration + inactivity_timeout.mul_f64(1.1);
    // Shared clock: every device derives "am I active or paused right now"
    // fresh each tick from the same `start` (elapsed % cycle_duration),
    // rather than each device accumulating its own independent phase-timer
    // loop. The latter drifted the three devices out of sync over many
    // cycles, because 1.1x the timeout is not a whole multiple of the 1s
    // tick interval, so each device's own loop rounded to a slightly
    // different number of ticks per pause - an error that compounded cycle
    // over cycle. Computing the phase fresh from an absolute start time has
    // nothing to accumulate.
    //
    // `start` is also used below as the anchor for every device's tick
    // schedule (via `interval_at`, not plain `interval`), so all three
    // devices tick at the exact same wall-clock instants instead of each
    // starting its own 1s cadence from whenever its task happened to get
    // scheduled - otherwise a small constant per-device offset (up to one
    // tick) eats into the pause window's margin and can skip a reset on
    // some cycles even without any drift.
    let start = tokio::time::Instant::now();

    let mut handles = Vec::new();
    for device in devices {
        let tx = tx.clone();
        handles.push(tokio::spawn(run_device(
            device,
            tx,
            start,
            active_duration,
            cycle_duration,
        )));
    }

    for handle in handles {
        let _ = handle.await;
    }
    Ok(())
}

async fn run_device(
    mut device: FakeDevice,
    tx: mpsc::Sender<DeviceUpdate>,
    start: tokio::time::Instant,
    active_duration: Duration,
    cycle_duration: Duration,
) {
    let mut interval = tokio::time::interval_at(start, device.tick_interval);
    loop {
        interval.tick().await;

        let pos_in_cycle = start.elapsed().as_secs_f64() % cycle_duration.as_secs_f64();
        let moving = pos_in_cycle < active_duration.as_secs_f64();

        let speed_mps = if moving {
            let elapsed_s = device.tick_interval.as_secs_f64();
            let jitter = rand::thread_rng().gen_range(0.7..1.3);
            device.distance_m += device.speed_mps as f64 * elapsed_s * jitter;
            device.speed_mps
        } else {
            0.0
        };

        let update = DeviceUpdate {
            device_id: device.device_id,
            device_type: device.device_type,
            distance_m: device.distance_m,
            speed_mps: Some(speed_mps),
            heart_rate: None,
            timestamp: Instant::now(),
        };
        if tx.send(update).await.is_err() {
            return; // receiver gone, shut this device down
        }
    }
}
