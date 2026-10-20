use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::types::{DeviceUpdate, EquipmentType};

struct FakeDevice {
    device_id: u16,
    device_type: EquipmentType,
    distance_m: f64,
    speed_mps: f32,
    tick_interval: Duration,
}

/// Sets per simulated session, each `WORK` long followed by `SHORT_REST`.
const SETS_PER_SESSION: u32 = 3;
const WORK: Duration = Duration::from_secs(40);
const SHORT_REST: Duration = Duration::from_secs(20);
/// Fraction of working speed devices keep reporting while resting. Some
/// real machines do this (a spinning-down flywheel, handles drifting), and
/// it's what set detection has to see through.
const REST_TRICKLE: f64 = 0.05;

/// Mirrors `ant_source::usb::run`'s signature so `main.rs` can pick either
/// source without the rest of the pipeline (state/ws) knowing the
/// difference.
pub async fn run(tx: mpsc::Sender<DeviceUpdate>, session_timeout: Duration) -> anyhow::Result<()> {
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

    // A few sets with short trickling rests, then a rest long enough to end
    // the session, so `--simulate` exercises both reset levels.
    let session_cycle = (WORK + SHORT_REST) * SETS_PER_SESSION + session_timeout.mul_f64(1.1);
    // Devices derive their phase from, and tick on, the shared `start` so
    // they stay in sync.
    let start = tokio::time::Instant::now();

    let mut handles = Vec::new();
    for device in devices {
        let tx = tx.clone();
        handles.push(tokio::spawn(run_device(device, tx, start, session_cycle)));
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
    session_cycle: Duration,
) {
    let mut interval = tokio::time::interval_at(start, device.tick_interval);
    loop {
        interval.tick().await;

        let effort = if is_working(start.elapsed(), session_cycle) {
            1.0
        } else {
            REST_TRICKLE
        };
        let jitter = rand::random_range(0.7..1.3);
        let speed_mps = device.speed_mps as f64 * effort * jitter;
        device.distance_m += speed_mps * device.tick_interval.as_secs_f64();

        let update = DeviceUpdate {
            device_id: device.device_id,
            device_type: device.device_type,
            distance_m: device.distance_m,
            timestamp: Instant::now(),
        };
        if tx.send(update).await.is_err() {
            return; // receiver gone, shut this device down
        }
    }
}

fn is_working(elapsed: Duration, session_cycle: Duration) -> bool {
    let set_cycle = (WORK + SHORT_REST).as_secs_f64();
    let in_session = elapsed.as_secs_f64() % session_cycle.as_secs_f64();
    in_session < set_cycle * SETS_PER_SESSION as f64 && in_session % set_cycle < WORK.as_secs_f64()
}
