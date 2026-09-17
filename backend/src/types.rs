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
        // ANT+ FE-C equipment type field, low 5 bits.
        match byte & 0x1F {
            25 => EquipmentType::Bike,
            22 => EquipmentType::Rower,
            21 => EquipmentType::NordicSkier,
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
    pub distance_m: f64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    State {
        total_distance_m: f64,
        /// `None` when no session is currently running - either at startup
        /// before any device has ever reported, or right after an
        /// inactivity reset before new data has arrived. Distinct from a
        /// real epoch-ms timestamp so the frontend can't mistake "no
        /// session yet" for "session started at the Unix epoch".
        session_started_at_ms: Option<u64>,
        devices: Vec<DeviceRow>,
    },
}
