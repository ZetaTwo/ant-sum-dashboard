// Hand-mirrored from backend/src/types.rs (WsMessage/DeviceRow). No shared
// codegen at this scale — keep these in sync manually if the Rust shape
// changes.

export interface DeviceRow {
  device_id: number;
  device_type: "bike" | "rower" | "nordic_skier" | "other";
  distance_m: number;
  last_seen_ms: number;
}

export interface StateMessage {
  type: "state";
  total_distance_m: number;
  // null when no session is currently running (before any device has ever
  // reported, or right after an inactivity reset) - serde's Option<u64>.
  session_started_at_ms: number | null;
  devices: DeviceRow[];
}
