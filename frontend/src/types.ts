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
  session_started_at_ms: number;
  devices: DeviceRow[];
}
