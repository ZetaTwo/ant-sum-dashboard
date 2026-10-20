use std::time::Instant;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentType {
    Bike,
    Rower,
    NordicSkier,
    Other(u8),
}

impl EquipmentType {
    pub fn from_fe_c_byte(byte: u8) -> Self {
        // ANT+ FE-C equipment type field, low 5 bits. Verified against the
        // ANT+ Device Profile - Fitness Equipment spec, Table 8-8: 19
        // Treadmill, 20 Elliptical, 21 Reserved (Do Not Use), 22 Rower, 23
        // Climber, 24 Nordic Skier, 25 Trainer/Stationary Bike. (21 was
        // previously mismapped here to NordicSkier - that's the reserved
        // value, the real Nordic Skier code is 24.)
        match byte & 0x1F {
            25 => EquipmentType::Bike,
            22 => EquipmentType::Rower,
            24 => EquipmentType::NordicSkier,
            other => EquipmentType::Other(other),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EquipmentType::Bike => "bike",
            EquipmentType::Rower => "rower",
            EquipmentType::NordicSkier => "nordic_skier",
            EquipmentType::Other(_) => "other",
        }
    }
}

/// The single shape both the real ANT+ USB source and the simulator produce.
/// Downstream aggregation/session/websocket code only ever sees this type,
/// so it has no idea whether data is real or simulated.
#[derive(Debug, Clone)]
pub struct DeviceUpdate {
    pub device_id: u16,
    pub device_type: EquipmentType,
    /// Cumulative distance in meters, already rollover-corrected, monotonic
    /// for the lifetime of the process.
    pub distance_m: f64,
    pub speed_mps: Option<f32>,
    pub heart_rate: Option<u8>,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceRow {
    pub device_id: u16,
    pub device_type: &'static str,
    pub set_distance_m: f64,
    pub session_distance_m: f64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetView {
    pub distance_m: f64,
    pub started_at_ms: u64,
    /// `None` while the set is running. Once it ends, the set stays on
    /// screen with its final numbers until the next set starts.
    pub ended_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub distance_m: f64,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    State {
        /// The current or most recent set of this session; `None` when no
        /// session is running.
        set: Option<SetView>,
        /// `None` until the first set starts, and again after the session
        /// times out.
        session: Option<SessionView>,
        devices: Vec<DeviceRow>,
    },
}
