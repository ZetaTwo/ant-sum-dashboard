//! Real ANT+ USB dongle data source, validated in a throwaway spike against
//! the attached ANTUSB-m stick (VID:PID 0x0fcf:0x1009) using the `ant`
//! crate (git dependency, `cujomalainey/ant-rs`, `development` branch -
//! crates.io's `ant`/`ant-usb`/`ant-plus` are stale 2017 placeholders and
//! must not be used).
//!
//! `ant-rs` has no FE-C (Fitness Equipment) profile, so this module talks
//! to the radio at the raw message level and hands 8-byte broadcast
//! payloads to `crate::fe_c::parse_page16` ourselves - the same parser used
//! in tests. Multiple devices are received on a single channel via ANT+
//! continuous scan mode (`OpenRxScanMode`), with extended broadcast
//! messages (`EnableExtRxMessages`) carrying each payload's originating
//! device number/type so devices can be told apart without pre-pairing.
//!
//! The driver is used directly rather than through `ant::router::Router`:
//! the router collapses every `DriverError` into a detail-less
//! `RouterError::DriverError()`, which makes a single corrupt frame
//! indistinguishable from the dongle disappearing. Talking to the driver
//! keeps `DriverError::SystemError` (USB I/O failure, reconnect) apart from
//! frame parse errors (the driver has already skipped past the bad byte,
//! so just keep reading).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ant::drivers::{Driver, DriverError, UsbDriver, is_ant_usb_device_from_device};
use ant::messages::RxMessage;
use ant::messages::config::{
    AssignChannel, ChannelId, ChannelRfFrequency, ChannelType, EnableExtRxMessages, SetNetworkKey,
};
use ant::messages::control::{OpenRxScanMode, RequestMessage, RequestableMessageId, ResetSystem};
use rusb::{DeviceList, GlobalContext};
use tokio::sync::mpsc;

use crate::fe_c::{DistanceAccumulator, parse_page16};
use crate::types::{DeviceUpdate, EquipmentType};

/// 2400 + 57 = 2457 MHz, the standard ANT+ operating frequency.
const ANT_PLUS_RF_FREQUENCY_OFFSET: u8 = 57;
/// ANT+ FE-C (Fitness Equipment) device type, per the ANT+ device profile.
const FE_C_DEVICE_TYPE: u8 = 0x11;
/// The single scan-mode channel everything is received on.
const CHANNEL: u8 = 0;

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

/// How long to wait for the stick to answer the capabilities request sent
/// right after a reset.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(1);

/// The stick only talks when a device is in range, so silence alone can't
/// tell "nobody's here" from "the stick has stopped responding" (e.g. after
/// the host resumes from sleep). A channel status request every
/// `PROBE_INTERVAL` guarantees some traffic; none at all for
/// `STALL_TIMEOUT` means the connection is dead and gets rebuilt.
const PROBE_INTERVAL: Duration = Duration::from_secs(5);
const STALL_TIMEOUT: Duration = Duration::from_secs(15);

type Accumulators = HashMap<u16, DistanceAccumulator>;

pub async fn run(tx: mpsc::Sender<DeviceUpdate>) -> anyhow::Result<()> {
    // Outlives individual connections: a fresh accumulator would restart
    // that device's total at 0 m after every reconnect. Kept, the first
    // reading after a reconnect is measured against the last one before
    // it, so distance covered during the gap still counts.
    let mut accumulators = Accumulators::new();
    loop {
        let attempt_tx = tx.clone();
        // ant-rs's driver is a synchronous polling API, so it runs on a
        // blocking thread and hands results back over the async channel.
        let (result, returned) = tokio::task::spawn_blocking(move || {
            let result = run_blocking(attempt_tx, &mut accumulators);
            (result, accumulators)
        })
        .await
        .map_err(|e| anyhow::anyhow!("ANT+ USB thread panicked: {e}"))?;
        accumulators = returned;

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

fn run_blocking(
    tx: mpsc::Sender<DeviceUpdate>,
    accumulators: &mut Accumulators,
) -> anyhow::Result<()> {
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

    let mut driver = UsbDriver::new(device)
        .map_err(|e| anyhow::anyhow!("failed to open ANT+ USB driver: {e:?}"))?;

    reset(&mut driver)?;

    send(&mut driver, &SetNetworkKey::new(0, ANT_PLUS_NETWORK_KEY))?;
    send(
        &mut driver,
        &AssignChannel::new(CHANNEL, ChannelType::BidirectionalSlave, 0, None),
    )?;
    send(&mut driver, &ChannelId::new_wildcard(CHANNEL))?;
    send(
        &mut driver,
        &ChannelRfFrequency::new(CHANNEL, ANT_PLUS_RF_FREQUENCY_OFFSET),
    )?;
    send(&mut driver, &EnableExtRxMessages::new(true))?;
    send(&mut driver, &OpenRxScanMode::new(None))?;

    tracing::info!("ANT+ USB radio initialized, listening in continuous scan mode");

    let mut last_rx = Instant::now();
    let mut last_probe = Instant::now();

    loop {
        while let Some(message) = next_message(&mut driver)? {
            last_rx = Instant::now();
            let Some(update) = to_device_update(&message, accumulators) else {
                continue;
            };
            if tx.blocking_send(update).is_err() {
                return Ok(()); // receiver gone, shut down
            }
        }

        if last_rx.elapsed() >= STALL_TIMEOUT {
            anyhow::bail!("no response from ANT+ USB stick for {STALL_TIMEOUT:?}");
        }
        if last_probe.elapsed() >= PROBE_INTERVAL {
            send(
                &mut driver,
                &RequestMessage::new(CHANNEL, RequestableMessageId::ChannelStatus, None),
            )?;
            last_probe = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Resets the radio and waits for it to come back, mirroring the handshake
/// `ant::router::Router::new` performs: reset, purge whatever was already
/// queued, then use the capabilities response as the "ready" signal.
fn reset(driver: &mut UsbDriver<GlobalContext>) -> anyhow::Result<()> {
    send(driver, &ResetSystem::new())?;
    while driver.get_message().unwrap_or(None).is_some() {}
    send(
        driver,
        &RequestMessage::new(0, RequestableMessageId::Capabilities, None),
    )?;

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(message) = next_message(driver)?
            && matches!(message, RxMessage::Capabilities(_))
        {
            return Ok(());
        }
    }
    anyhow::bail!("ANT+ USB stick did not respond after reset")
}

/// Returns the next complete message, `None` once the driver's buffer is
/// drained, or an error if the USB connection itself failed.
fn next_message(driver: &mut UsbDriver<GlobalContext>) -> anyhow::Result<Option<RxMessage>> {
    loop {
        match driver.get_message() {
            Ok(message) => return Ok(message.map(|m| m.message)),
            Err(DriverError::SystemError(err)) => {
                anyhow::bail!("ANT+ USB read failed: {err:?}")
            }
            // A malformed frame (bad checksum, unknown message id, ...).
            // The driver has already dropped the offending byte and will
            // resynchronize on the next sync byte, so keep reading.
            Err(err) => tracing::warn!(?err, "skipping malformed ANT+ message"),
        }
    }
}

fn send(
    driver: &mut UsbDriver<GlobalContext>,
    message: &dyn ant::messages::TransmitableMessage,
) -> anyhow::Result<()> {
    driver
        .send_message(message)
        .map_err(|e| anyhow::anyhow!("ANT+ USB write failed: {e:?}"))
}

fn to_device_update(message: &RxMessage, accumulators: &mut Accumulators) -> Option<DeviceUpdate> {
    let RxMessage::BroadcastData(data) = message else {
        return None;
    };
    // No device id info means it can't be attributed to a device.
    let channel_id = data.extended_info.and_then(|ext| ext.channel_id_output)?;
    let device_type_id: u8 = channel_id.device_type.device_type_id.into();
    if device_type_id != FE_C_DEVICE_TYPE {
        return None;
    }
    // Only the General FE Data page carries distance.
    let page = parse_page16(&data.payload.data)?;
    tracing::debug!(
        device_number = channel_id.device_number,
        equipment_type_byte = page.equipment_type_byte,
        distance_raw = ?page.distance_raw,
        speed_mps = ?page.speed_mps,
        fe_state = ?page.fe_state,
        "page16 parsed"
    );
    // Devices that don't report distance are of no use here.
    let distance_raw = page.distance_raw?;

    let device_id = channel_id.device_number;
    let distance_m = accumulators
        .entry(device_id)
        .or_default()
        .accumulate(distance_raw);

    Some(DeviceUpdate {
        device_id,
        device_type: EquipmentType::from_fe_c_byte(page.equipment_type_byte),
        distance_m,
        speed_mps: page.speed_mps,
        heart_rate: page.heart_rate,
        timestamp: Instant::now(),
    })
}
