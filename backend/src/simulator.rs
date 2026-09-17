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
pub async fn run(tx: mpsc::Sender<DeviceUpdate>) -> anyhow::Result<()> {
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

    let mut handles = Vec::new();
    for mut device in devices {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(device.tick_interval);
            loop {
                interval.tick().await;
                let elapsed_s = device.tick_interval.as_secs_f64();
                let jitter = rand::thread_rng().gen_range(0.7..1.3);
                device.distance_m += device.speed_mps as f64 * elapsed_s * jitter;

                let update = DeviceUpdate {
                    device_id: device.device_id,
                    device_type: device.device_type,
                    distance_m: device.distance_m,
                    speed_mps: Some(device.speed_mps),
                    heart_rate: None,
                    timestamp: Instant::now(),
                };
                if tx.send(update).await.is_err() {
                    break; // receiver gone, shut this device down
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
    Ok(())
}
