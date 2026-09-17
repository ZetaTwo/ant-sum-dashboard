//! ANT+ FE-C (Fitness Equipment) "General FE Data" broadcast page (page
//! number 16 / 0x10) parsing, and per-device distance rollover accumulation.
//!
//! Byte layout verified against the ANT+ Device Profile - Fitness Equipment
//! spec (Tables 8-8/8-9, via a mirrored copy of D000001231 Rev 5.0 - not
//! read directly from thisisant.com) against real hardware: page number,
//! equipment type (bits 0-4 of byte 1), elapsed time (byte 2, 0.25s,
//! rolls over at 64s - `elapsed_time_raw` is stored but not currently
//! consumed), distance (byte 3, 1m, rolls over at 256m), speed (bytes 4-5,
//! little-endian, 0.001 m/s, 0xFFFF = invalid), heart rate (byte 6, 0xFF =
//! invalid), and the distance-enabled bit (byte 7, bit 2).

const PAGE_NUMBER_GENERAL_FE_DATA: u8 = 0x10;
const DISTANCE_TRAVELED_ENABLED_BIT: u8 = 0x04;

/// Byte 7, bits 4-6 of the General FE Data page: what the equipment itself
/// thinks its current state is. Lets us tell "device is idle/not in use, so
/// frozen readings are expected" apart from "device is in use but we're not
/// seeing fresh data" - the latter would be a real bug.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeState {
    Asleep,
    Ready,
    InUse,
    Finished,
    Unknown(u8),
}

impl FeState {
    fn from_bits(bits: u8) -> Self {
        match bits {
            1 => FeState::Asleep,
            2 => FeState::Ready,
            3 => FeState::InUse,
            4 => FeState::Finished,
            other => FeState::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Page16Data {
    pub equipment_type_byte: u8,
    pub elapsed_time_raw: u8,
    /// Only `Some` when the source device sets the "distance traveled
    /// enabled" capability bit; otherwise the raw byte is not meaningful.
    pub distance_raw: Option<u8>,
    pub speed_mps: Option<f32>,
    pub heart_rate: Option<u8>,
    pub fe_state: FeState,
}

/// Parses an 8-byte FE-C broadcast payload as page 16 ("General FE Data").
/// Returns `None` if the payload isn't page 16.
pub fn parse_page16(payload: &[u8; 8]) -> Option<Page16Data> {
    if payload[0] != PAGE_NUMBER_GENERAL_FE_DATA {
        return None;
    }

    let capability_flags = payload[7];
    let distance_enabled = capability_flags & DISTANCE_TRAVELED_ENABLED_BIT != 0;

    let speed_raw = u16::from_le_bytes([payload[4], payload[5]]);
    let speed_mps = match speed_raw {
        0xFFFF => None,
        raw => Some(raw as f32 / 1000.0),
    };
    let heart_rate = match payload[6] {
        0xFF => None,
        hr => Some(hr),
    };

    Some(Page16Data {
        equipment_type_byte: payload[1],
        elapsed_time_raw: payload[2],
        distance_raw: distance_enabled.then_some(payload[3]),
        speed_mps,
        heart_rate,
        fe_state: FeState::from_bits((capability_flags >> 4) & 0x07),
    })
}

/// Tracks a single device's rolling 1-byte (0-255m) distance counter and
/// unrolls it into a monotonically increasing total.
#[derive(Debug, Default)]
pub struct DistanceAccumulator {
    total_m: f64,
    last_raw_byte: Option<u8>,
}

impl DistanceAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one raw distance byte, returning the updated cumulative total.
    /// The first byte received establishes a baseline and adds nothing.
    pub fn accumulate(&mut self, raw_byte: u8) -> f64 {
        if let Some(last) = self.last_raw_byte {
            let delta = if raw_byte >= last {
                (raw_byte - last) as f64
            } else {
                // Byte wrapped past 255 back to 0. Any decrease is treated
                // as a rollover; a device resetting mid-session or a
                // corrupt/duplicate packet could cause a spurious jump, but
                // basic single-rollover detection is what the FE-C spec
                // expects here.
                (256 - last as u32 + raw_byte as u32) as f64
            };
            self.total_m += delta;
        }
        self.last_raw_byte = Some(raw_byte);
        self.total_m
    }

    pub fn total_m(&self) -> f64 {
        self.total_m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_page16_rejects_other_pages() {
        let payload = [0x01, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(parse_page16(&payload), None);
    }

    #[test]
    fn parse_page16_extracts_fields() {
        // page=0x10, equipment=25 (bike), elapsed=10, distance=42,
        // speed=1000 (1.0 m/s) little-endian, hr=150, flags=distance-enabled
        let payload = [
            0x10,
            25,
            10,
            42,
            0xE8,
            0x03,
            150,
            DISTANCE_TRAVELED_ENABLED_BIT,
        ];
        let parsed = parse_page16(&payload).unwrap();
        assert_eq!(parsed.equipment_type_byte, 25);
        assert_eq!(parsed.elapsed_time_raw, 10);
        assert_eq!(parsed.distance_raw, Some(42));
        assert_eq!(parsed.speed_mps, Some(1.0));
        assert_eq!(parsed.heart_rate, Some(150));
    }

    #[test]
    fn parse_page16_treats_0xffff_speed_as_invalid() {
        let payload = [0x10, 25, 10, 42, 0xFF, 0xFF, 150, DISTANCE_TRAVELED_ENABLED_BIT];
        let parsed = parse_page16(&payload).unwrap();
        assert_eq!(parsed.speed_mps, None);
    }

    #[test]
    fn parse_page16_ignores_distance_when_capability_bit_unset() {
        let payload = [0x10, 25, 10, 42, 0, 0, 0xFF, 0x00];
        let parsed = parse_page16(&payload).unwrap();
        assert_eq!(parsed.distance_raw, None);
        assert_eq!(parsed.heart_rate, None);
    }

    #[test]
    fn accumulator_first_byte_adds_nothing() {
        let mut acc = DistanceAccumulator::new();
        assert_eq!(acc.accumulate(100), 0.0);
    }

    #[test]
    fn accumulator_normal_increment() {
        let mut acc = DistanceAccumulator::new();
        acc.accumulate(10);
        assert_eq!(acc.accumulate(50), 40.0);
    }

    #[test]
    fn accumulator_repeated_byte_is_a_no_op() {
        let mut acc = DistanceAccumulator::new();
        acc.accumulate(255);
        assert_eq!(acc.accumulate(255), 0.0);
    }

    #[test]
    fn accumulator_handles_exact_rollover() {
        let mut acc = DistanceAccumulator::new();
        acc.accumulate(255);
        // 255 -> 0 is a 1m step (255, then wraps to 0).
        assert_eq!(acc.accumulate(0), 1.0);
    }

    #[test]
    fn accumulator_handles_rollover_with_remainder() {
        let mut acc = DistanceAccumulator::new();
        acc.accumulate(250);
        // 250 -> 10 after wrapping: (256 - 250) + 10 = 16
        assert_eq!(acc.accumulate(10), 16.0);
    }

    #[test]
    fn accumulator_multiple_rollovers_accumulate_correctly() {
        let mut acc = DistanceAccumulator::new();
        let sequence = [0u8, 100, 200, 255, 0, 50, 200, 255, 5, 5, 10];
        for b in sequence {
            acc.accumulate(b);
        }
        // 0->100 (100) ->200 (100) ->255 (55) ->0 (1) ->50 (50) ->200 (150)
        // ->255 (55) ->5 (6) ->5 (0) ->10 (5) = 522
        assert_eq!(acc.total_m(), 522.0);
    }
}
