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
    let pause_duration = inactivity_timeout.mul_f64(1.1);

    let mut handles = Vec::new();
    for device in devices {
        let tx = tx.clone();
        handles.push(tokio::spawn(run_device(
            device,
            tx,
            active_duration,
            pause_duration,
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
    active_duration: Duration,
    pause_duration: Duration,
) {
    let mut interval = tokio::time::interval(device.tick_interval);
    loop {
        if !run_phase(&mut device, &tx, &mut interval, active_duration, true).await {
            return; // receiver gone, shut this device down
        }
        if !run_phase(&mut device, &tx, &mut interval, pause_duration, false).await {
            return;
        }
    }
}

/// Ticks for `phase_duration`, sending one update per tick. When `moving`
/// is true, distance advances (with jitter) like a device in use; when
/// false, distance holds and speed reports 0 - a device that's connected
/// and still transmitting, but not being pedaled/rowed/skied.
async fn run_phase(
    device: &mut FakeDevice,
    tx: &mpsc::Sender<DeviceUpdate>,
    interval: &mut tokio::time::Interval,
    phase_duration: Duration,
    moving: bool,
) -> bool {
    let phase_start = Instant::now();
    while phase_start.elapsed() < phase_duration {
        interval.tick().await;

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
            return false; // receiver gone, shut this device down
        }
    }
    true
}
