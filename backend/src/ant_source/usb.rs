//! Real ANT+ USB dongle data source, validated in a throwaway spike against
//! the attached ANTUSB-m stick (VID:PID 0x0fcf:0x1009) using the `ant`
//! crate (git dependency, `cujomalainey/ant-rs`, `development` branch -
//! crates.io's `ant`/`ant-usb`/`ant-plus` are stale 2017 placeholders and
//! must not be used).
//!
//! `ant-rs` has no FE-C (Fitness Equipment) profile, so this module talks
//! to the radio at the raw message level (`Router`) and hands 8-byte
//! broadcast payloads to `crate::fe_c::parse_page16` ourselves - the same
//! parser used in tests. Multiple devices are received on a single channel
//! via ANT+ continuous scan mode (`OpenRxScanMode`), with extended
//! broadcast messages (`EnableExtRxMessages`) carrying each payload's
//! originating device number/type so devices can be told apart without
//! pre-pairing.

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use ant::channel::mpsc::{RxChannel, TxChannel};
use ant::drivers::{UsbDriver, is_ant_usb_device_from_device};
use ant::messages::RxMessage;
use ant::messages::config::{
    AssignChannel, ChannelId, ChannelRfFrequency, ChannelType, EnableExtRxMessages, SetNetworkKey,
};
use ant::messages::control::OpenRxScanMode;
use ant::router::Router;
use rusb::DeviceList;
use tokio::sync::mpsc;

use crate::fe_c::{DistanceAccumulator, parse_page16};
use crate::types::{DeviceUpdate, EquipmentType};

/// 2400 + 57 = 2457 MHz, the standard ANT+ operating frequency.
const ANT_PLUS_RF_FREQUENCY_OFFSET: u8 = 57;
/// ANT+ FE-C (Fitness Equipment) device type, per the ANT+ device profile.
const FE_C_DEVICE_TYPE: u8 = 0x11;

/// The ANT+ Managed Network key. Formally gated behind Garmin's ANT+
/// Adopter program registration, but this exact value is openly published
/// as an ordinary constant in mainstream open-source ANT+ implementations -
/// e.g. openant (`openant/devices/__init__.py`: `ANTPLUS_NETWORK_KEY`) and
/// GoldenCheetah (`src/ANT/ANT.cpp`: `ANT::key`). Required for any
/// ANT+ Device Profile broadcast (HR, speed/cadence, FE-C, ...) to sync -
/// there is no keyless "public" ANT+ mode.
const ANT_PLUS_NETWORK_KEY: [u8; 8] = [0xB9, 0xA5, 0x21, 0xFB, 0xBD, 0x72, 0xC3, 0x45];

/// Wait between reconnect attempts after a USB/protocol error - e.g. the
/// dongle dropping out, which happens routinely on WSL2 when the usbipd
/// passthrough drops. Fixed rather than exponential: attempts are cheap
/// (device enumeration, then either a fast failure or a fresh handshake),
/// so there's no real cost to just retrying at a steady pace while waiting
/// for the user to reattach the device.
const RETRY_DELAY: Duration = Duration::from_secs(5);

pub async fn run(tx: mpsc::Sender<DeviceUpdate>) -> anyhow::Result<()> {
    loop {
        let attempt_tx = tx.clone();
        // ant-rs's Router::process() is a synchronous polling loop, so it
        // runs on a blocking thread and hands results back over the async
        // channel.
        let result = tokio::task::spawn_blocking(move || run_blocking(attempt_tx))
            .await
            .map_err(|e| anyhow::anyhow!("ANT+ USB thread panicked: {e}"))?;

        match result {
            // `run_blocking` only returns `Ok(())` when the update
            // channel's receiver was dropped, meaning the app itself is
            // shutting down - nothing to retry.
            Ok(()) => return Ok(()),
            Err(err) => {
                tracing::warn!(
                    ?err,
                    retry_in_secs = RETRY_DELAY.as_secs(),
                    "ANT+ USB source failed, retrying"
                );
                tokio::time::sleep(RETRY_DELAY).await;
            }
        }
    }
}

fn run_blocking(tx: mpsc::Sender<DeviceUpdate>) -> anyhow::Result<()> {
    let device = DeviceList::new()?
        .iter()
        .find(is_ant_usb_device_from_device)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no ANT+ USB stick found - is it attached? (on WSL2, check `lsusb` and \
                 reattach via `usbipd attach --wsl` if it dropped)"
            )
        })?;

    let descriptor = device.device_descriptor().ok();
    tracing::info!(
        bus = device.bus_number(),
        address = device.address(),
        vendor_id = descriptor.as_ref().map(|d| d.vendor_id()),
        product_id = descriptor.as_ref().map(|d| d.product_id()),
        "ANT+ USB device detected, opening driver"
    );

    let driver = UsbDriver::new(device)
        .map_err(|e| anyhow::anyhow!("failed to open ANT+ USB driver: {e:?}"))?;

    let (_unused_tx, unused_rx) = std_mpsc::channel();
    let mut router = Router::new(
        driver,
        RxChannel {
            receiver: unused_rx,
        },
    )
    .map_err(|e| anyhow::anyhow!("Router::new failed: {e:?}"))?;

    let (chan_tx, chan_rx) = std_mpsc::channel();
    let channel_number = router
        .add_channel(TxChannel { sender: chan_tx })
        .map_err(|e| anyhow::anyhow!("add_channel failed: {e:?}"))?;

    router
        .send(&SetNetworkKey::new(0, ANT_PLUS_NETWORK_KEY))
        .map_err(|e| anyhow::anyhow!("SetNetworkKey failed: {e:?}"))?;
    router
        .send(&AssignChannel::new(
            channel_number,
            ChannelType::BidirectionalSlave,
            0,
            None,
        ))
        .map_err(|e| anyhow::anyhow!("AssignChannel failed: {e:?}"))?;
    router
        .send(&ChannelId::new_wildcard(channel_number))
        .map_err(|e| anyhow::anyhow!("ChannelId failed: {e:?}"))?;
    router
        .send(&ChannelRfFrequency::new(
            channel_number,
            ANT_PLUS_RF_FREQUENCY_OFFSET,
        ))
        .map_err(|e| anyhow::anyhow!("ChannelRfFrequency failed: {e:?}"))?;
    router
        .send(&EnableExtRxMessages::new(true))
        .map_err(|e| anyhow::anyhow!("EnableExtRxMessages failed: {e:?}"))?;
    router
        .send(&OpenRxScanMode::new(None))
        .map_err(|e| anyhow::anyhow!("OpenRxScanMode failed: {e:?}"))?;

    tracing::info!("ANT+ USB radio initialized, listening in continuous scan mode");

    let mut accumulators: HashMap<u16, DistanceAccumulator> = HashMap::new();

    loop {
        router
            .process()
            .map_err(|e| anyhow::anyhow!("router.process failed: {e:?}"))?;

        while let Ok(msg) = chan_rx.try_recv() {
            let RxMessage::BroadcastData(data) = &msg.message else {
                continue;
            };
            let Some(channel_id) = data.extended_info.and_then(|ext| ext.channel_id_output) else {
                continue; // no device id info, can't attribute this to a device
            };
            let device_type_id: u8 = channel_id.device_type.device_type_id.into();
            if device_type_id != FE_C_DEVICE_TYPE {
                continue;
            }
            let Some(page) = parse_page16(&data.payload.data) else {
                continue; // not the General FE Data page
            };
            tracing::debug!(
                device_number = channel_id.device_number,
                equipment_type_byte = page.equipment_type_byte,
                distance_raw = ?page.distance_raw,
                speed_mps = ?page.speed_mps,
                fe_state = ?page.fe_state,
                "page16 parsed"
            );
            let Some(distance_raw) = page.distance_raw else {
                continue; // device doesn't report distance
            };

            let device_id = channel_id.device_number;
            let accumulator = accumulators.entry(device_id).or_default();
            let distance_m = accumulator.accumulate(distance_raw);

            let update = DeviceUpdate {
                device_id,
                device_type: EquipmentType::from_fe_c_byte(page.equipment_type_byte),
                distance_m,
                speed_mps: page.speed_mps,
                heart_rate: page.heart_rate,
                timestamp: Instant::now(),
            };
            if tx.blocking_send(update).is_err() {
                return Ok(()); // receiver gone, shut down
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
