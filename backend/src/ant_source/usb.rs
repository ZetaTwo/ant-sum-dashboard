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
use std::fmt::Debug;
use std::time::{Duration, Instant};

use ant::drivers::{Driver, DriverError, UsbDriver, is_ant_usb_device_from_device};
use ant::messages::RxMessage;
use ant::messages::config::{
    AssignChannel, ChannelId, ChannelRfFrequency, ChannelType, EnableExtRxMessages, SetNetworkKey,
};
use ant::messages::control::{OpenRxScanMode, RequestMessage, RequestableMessageId, ResetSystem};
use rusb::DeviceList;
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

struct Timing {
    /// How long to wait for the stick to answer the capabilities request
    /// sent right after a reset.
    startup_timeout: Duration,
    /// The stick only talks when a device is in range, so silence alone
    /// can't tell "nobody's here" from "the stick has stopped responding"
    /// (e.g. after the host resumes from sleep). A channel status request
    /// every `probe_interval` guarantees some traffic; none at all for
    /// `stall_timeout` means the connection is dead and gets rebuilt.
    probe_interval: Duration,
    stall_timeout: Duration,
    /// Pause between polls once the driver has nothing buffered.
    poll_interval: Duration,
}

const TIMING: Timing = Timing {
    startup_timeout: Duration::from_secs(1),
    probe_interval: Duration::from_secs(5),
    stall_timeout: Duration::from_secs(15),
    poll_interval: Duration::from_millis(10),
};

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

    run_connection(&mut driver, &tx, accumulators, &TIMING)
}

/// Configures an opened stick for scanning and forwards FE-C distance
/// updates until the update receiver is dropped (`Ok`) or the connection
/// fails (`Err`). Generic over the driver so tests can run it against a
/// scripted serial port.
fn run_connection<E: Debug, D: Driver<E>>(
    driver: &mut D,
    tx: &mpsc::Sender<DeviceUpdate>,
    accumulators: &mut Accumulators,
    timing: &Timing,
) -> anyhow::Result<()> {
    reset(driver, timing)?;

    send(driver, &SetNetworkKey::new(0, ANT_PLUS_NETWORK_KEY))?;
    send(
        driver,
        &AssignChannel::new(CHANNEL, ChannelType::BidirectionalSlave, 0, None),
    )?;
    send(driver, &ChannelId::new_wildcard(CHANNEL))?;
    send(
        driver,
        &ChannelRfFrequency::new(CHANNEL, ANT_PLUS_RF_FREQUENCY_OFFSET),
    )?;
    send(driver, &EnableExtRxMessages::new(true))?;
    send(driver, &OpenRxScanMode::new(None))?;

    tracing::info!("ANT+ USB radio initialized, listening in continuous scan mode");

    let mut last_rx = Instant::now();
    let mut last_probe = Instant::now();

    loop {
        while let Some(message) = next_message(driver)? {
            last_rx = Instant::now();
            let Some(update) = to_device_update(&message, accumulators) else {
                continue;
            };
            if tx.blocking_send(update).is_err() {
                return Ok(()); // receiver gone, shut down
            }
        }

        if last_rx.elapsed() >= timing.stall_timeout {
            anyhow::bail!(
                "no response from ANT+ USB stick for {:?}",
                timing.stall_timeout
            );
        }
        if last_probe.elapsed() >= timing.probe_interval {
            send(
                driver,
                &RequestMessage::new(CHANNEL, RequestableMessageId::ChannelStatus, None),
            )?;
            last_probe = Instant::now();
        }

        std::thread::sleep(timing.poll_interval);
    }
}

/// Resets the radio and waits for it to come back, mirroring the handshake
/// `ant::router::Router::new` performs: reset, purge whatever was already
/// queued, then use the capabilities response as the "ready" signal.
fn reset<E: Debug, D: Driver<E>>(driver: &mut D, timing: &Timing) -> anyhow::Result<()> {
    send(driver, &ResetSystem::new())?;
    while driver.get_message().unwrap_or(None).is_some() {}
    send(
        driver,
        &RequestMessage::new(0, RequestableMessageId::Capabilities, None),
    )?;

    let deadline = Instant::now() + timing.startup_timeout;
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
fn next_message<E: Debug, D: Driver<E>>(driver: &mut D) -> anyhow::Result<Option<RxMessage>> {
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

fn send<E: Debug, D: Driver<E>>(
    driver: &mut D,
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
    let timestamp = Instant::now();
    let distance_m = accumulators
        .entry(device_id)
        .or_default()
        .accumulate(distance_raw, timestamp);

    Some(DeviceUpdate {
        device_id,
        device_type: EquipmentType::from_fe_c_byte(page.equipment_type_byte),
        distance_m,
        speed_mps: page.speed_mps,
        heart_rate: page.heart_rate,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    //! Runs `run_connection` through ant-rs's real `SerialDriver` (which
    //! shares its frame parsing with `UsbDriver`) on top of a fake stick
    //! speaking raw ANT wire bytes.

    use std::collections::VecDeque;

    use ant::drivers::{SerialDriver, StubPin};
    use embedded_hal_nb::nb;
    use embedded_hal_nb::serial::{self, ErrorKind, ErrorType, Read, Write};

    use super::*;

    const TEST_TIMING: Timing = Timing {
        startup_timeout: Duration::from_millis(100),
        probe_interval: Duration::from_millis(10),
        stall_timeout: Duration::from_millis(100),
        poll_interval: Duration::from_millis(1),
    };

    const SYNC: u8 = 0xA4;
    const MSG_BROADCAST_DATA: u8 = 0x4E;
    const MSG_CHANNEL_STATUS: u8 = 0x52;
    const MSG_CAPABILITIES: u8 = 0x54;
    const MSG_REQUEST: u8 = 0x4D;
    const MSG_OPEN_RX_SCAN_MODE: u8 = 0x5B;
    const EQUIPMENT_ROWER: u8 = 22;

    #[derive(Debug)]
    struct FakeIoError;

    impl serial::Error for FakeIoError {
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    enum Chunk {
        Bytes(Vec<u8>),
        /// Stay silent for this long before the next chunk.
        Silence(Duration),
        /// The read fails, like the dongle being unplugged.
        Fail,
    }

    /// Answers the host's reset and probe requests like a real stick, and
    /// plays `script` once the host has opened scan mode. Each chunk is
    /// followed by a `WouldBlock`, so the driver sees one chunk per read
    /// burst, as from a real port.
    struct FakeStick {
        answers_reset: bool,
        answers_probes: bool,
        script: VecDeque<Chunk>,
        replies: VecDeque<u8>,
        current: VecDeque<u8>,
        end_of_burst: bool,
        silent_until: Option<Instant>,
        scanning: bool,
        written: Vec<u8>,
    }

    impl FakeStick {
        fn new(script: Vec<Chunk>) -> Self {
            Self {
                answers_reset: true,
                answers_probes: true,
                script: script.into(),
                replies: VecDeque::new(),
                current: VecDeque::new(),
                end_of_burst: false,
                silent_until: None,
                scanning: false,
                written: Vec::new(),
            }
        }
    }

    impl ErrorType for FakeStick {
        type Error = FakeIoError;
    }

    impl Read<u8> for FakeStick {
        fn read(&mut self) -> nb::Result<u8, FakeIoError> {
            if let Some(byte) = self
                .replies
                .pop_front()
                .or_else(|| self.current.pop_front())
            {
                return Ok(byte);
            }
            if std::mem::take(&mut self.end_of_burst)
                || !self.scanning
                || self
                    .silent_until
                    .is_some_and(|until| Instant::now() < until)
            {
                return Err(nb::Error::WouldBlock);
            }
            match self.script.pop_front() {
                Some(Chunk::Bytes(bytes)) => {
                    self.current = bytes.into();
                    self.end_of_burst = true;
                    Ok(self.current.pop_front().expect("chunks are non-empty"))
                }
                Some(Chunk::Silence(duration)) => {
                    self.silent_until = Some(Instant::now() + duration);
                    Err(nb::Error::WouldBlock)
                }
                Some(Chunk::Fail) => Err(nb::Error::Other(FakeIoError)),
                None => Err(nb::Error::WouldBlock),
            }
        }
    }

    impl Write<u8> for FakeStick {
        fn write(&mut self, byte: u8) -> nb::Result<(), FakeIoError> {
            self.written.push(byte);
            Ok(())
        }

        /// The driver flushes after each complete outgoing frame.
        fn flush(&mut self) -> nb::Result<(), FakeIoError> {
            let frame = std::mem::take(&mut self.written);
            match (frame[2], frame.get(4)) {
                (MSG_REQUEST, Some(&MSG_CAPABILITIES)) if self.answers_reset => {
                    // 8 channels, 3 networks, no option flags.
                    self.replies
                        .extend(frame_of(MSG_CAPABILITIES, &[8, 3, 0, 0]));
                }
                (MSG_REQUEST, Some(&MSG_CHANNEL_STATUS)) if self.answers_probes => {
                    // Channel 0, bidirectional slave on network 0, searching.
                    self.replies
                        .extend(frame_of(MSG_CHANNEL_STATUS, &[CHANNEL, 0x02]));
                }
                (MSG_OPEN_RX_SCAN_MODE, _) => self.scanning = true,
                _ => {}
            }
            Ok(())
        }
    }

    fn frame_of(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![SYNC, payload.len() as u8, id];
        frame.extend_from_slice(payload);
        frame.push(frame.iter().fold(0, |acc, b| acc ^ b));
        frame
    }

    /// An extended broadcast (payload plus channel id) from `device`.
    fn broadcast(device: u16, device_type: u8, page: [u8; 8]) -> Vec<u8> {
        let mut payload = vec![CHANNEL];
        payload.extend_from_slice(&page);
        payload.push(0x80); // flag byte: channel id follows
        payload.extend_from_slice(&device.to_le_bytes());
        payload.extend_from_slice(&[device_type, 0x05]);
        frame_of(MSG_BROADCAST_DATA, &payload)
    }

    /// FE-C General FE Data page from a rower, in use, distance enabled.
    fn page16(distance: u8) -> [u8; 8] {
        [0x10, EQUIPMENT_ROWER, 0, distance, 0xE8, 0x03, 0xFF, 0x34]
    }

    fn fe_c(device: u16, distance: u8) -> Chunk {
        Chunk::Bytes(broadcast(device, FE_C_DEVICE_TYPE, page16(distance)))
    }

    fn run(stick: FakeStick, accumulators: &mut Accumulators) -> (anyhow::Result<()>, Vec<f64>) {
        let mut driver = SerialDriver::<_, StubPin>::new(stick, None);
        let (tx, mut rx) = mpsc::channel(64);
        let result = run_connection(&mut driver, &tx, accumulators, &TEST_TIMING);
        let mut distances = Vec::new();
        while let Ok(update) = rx.try_recv() {
            assert_eq!(update.device_type, EquipmentType::Rower);
            distances.push(update.distance_m);
        }
        (result, distances)
    }

    fn assert_err_contains(result: &anyhow::Result<()>, needle: &str) {
        let err = result.as_ref().expect_err("connection should have failed");
        assert!(
            err.to_string().contains(needle),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn forwards_fe_c_distance_updates() {
        let stick = FakeStick::new(vec![fe_c(7, 10), fe_c(7, 11), Chunk::Fail]);
        let (result, distances) = run(stick, &mut Accumulators::new());
        assert_eq!(distances, [0.0, 1.0]);
        assert_err_contains(&result, "read failed");
    }

    #[test]
    fn skips_malformed_frames_without_reconnecting() {
        let mut bad_checksum = broadcast(7, FE_C_DEVICE_TYPE, page16(11));
        *bad_checksum.last_mut().unwrap() ^= 0xFF;
        // Legacy extended broadcast, an id ant-rs doesn't know.
        let unknown_id = frame_of(0x5D, &[CHANNEL, 0, 0, 0, 0, 0, 0, 0, 0]);
        let stick = FakeStick::new(vec![
            fe_c(7, 10),
            Chunk::Bytes(bad_checksum),
            Chunk::Bytes(unknown_id),
            fe_c(7, 11),
            Chunk::Fail,
        ]);
        let (result, distances) = run(stick, &mut Accumulators::new());
        // Reaching the update after the bad frames, and only then failing
        // on the injected I/O error, shows they didn't end the connection.
        assert_eq!(distances, [0.0, 1.0]);
        assert_err_contains(&result, "read failed");
    }

    #[test]
    fn ignores_broadcasts_without_usable_distance() {
        let mut no_distance = page16(10);
        no_distance[7] &= !0x04;
        let mut other_page = page16(10);
        other_page[0] = 0x19;
        let mut without_channel_id = vec![CHANNEL];
        without_channel_id.extend_from_slice(&page16(10));
        let heart_rate_monitor = 0x78;
        let stick = FakeStick::new(vec![
            Chunk::Bytes(broadcast(7, heart_rate_monitor, page16(10))),
            Chunk::Bytes(broadcast(7, FE_C_DEVICE_TYPE, no_distance)),
            Chunk::Bytes(broadcast(7, FE_C_DEVICE_TYPE, other_page)),
            Chunk::Bytes(frame_of(MSG_BROADCAST_DATA, &without_channel_id)),
            fe_c(8, 10),
            Chunk::Fail,
        ]);
        let (_, distances) = run(stick, &mut Accumulators::new());
        assert_eq!(distances, [0.0]);
    }

    #[test]
    fn distance_carries_over_a_reconnect() {
        let mut accumulators = Accumulators::new();
        let first = FakeStick::new(vec![fe_c(7, 10), fe_c(7, 11), Chunk::Fail]);
        let (_, before) = run(first, &mut accumulators);
        let second = FakeStick::new(vec![fe_c(7, 12), Chunk::Fail]);
        let (_, after) = run(second, &mut accumulators);
        assert_eq!(before, [0.0, 1.0]);
        assert_eq!(after, [2.0]);
    }

    #[test]
    fn answered_probes_keep_a_quiet_connection_alive() {
        let stick = FakeStick::new(vec![
            Chunk::Silence(TEST_TIMING.stall_timeout * 3),
            fe_c(7, 10),
            Chunk::Fail,
        ]);
        let (result, distances) = run(stick, &mut Accumulators::new());
        assert_eq!(distances, [0.0]);
        assert_err_contains(&result, "read failed");
    }

    #[test]
    fn reconnects_when_the_stick_stops_answering() {
        let mut stick = FakeStick::new(vec![
            Chunk::Silence(TEST_TIMING.stall_timeout * 3),
            fe_c(7, 10),
        ]);
        stick.answers_probes = false;
        let (result, distances) = run(stick, &mut Accumulators::new());
        assert!(distances.is_empty());
        assert_err_contains(&result, "no response");
    }

    #[test]
    fn fails_when_the_stick_does_not_come_back_after_reset() {
        let mut stick = FakeStick::new(Vec::new());
        stick.answers_reset = false;
        let (result, _) = run(stick, &mut Accumulators::new());
        assert_err_contains(&result, "did not respond after reset");
    }

    #[test]
    fn stops_cleanly_when_the_receiver_is_gone() {
        let stick = FakeStick::new(vec![fe_c(7, 10)]);
        let mut driver = SerialDriver::<_, StubPin>::new(stick, None);
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let result = run_connection(&mut driver, &tx, &mut Accumulators::new(), &TEST_TIMING);
        assert!(result.is_ok());
    }
}
