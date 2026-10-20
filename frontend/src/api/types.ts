// Mirrors the Rust types in backend/src/types.rs (WsMessage and friends) by
// convention — hand-written, not generated; keep these in sync manually if
// the Rust shape changes. Field names stay snake_case, matching serde.

export interface DeviceRow {
  device_id: number;
  device_type: "bike" | "rower" | "nordic_skier" | "other";
  set_distance_m: number;
  session_distance_m: number;
  last_seen_ms: number;
}

export interface SetView {
  distance_m: number;
  started_at_ms: number;
  // null while the set is running; a finished set stays on screen until
  // the next one starts.
  ended_at_ms: number | null;
}

export interface SessionView {
  distance_m: number;
  started_at_ms: number;
}

export interface StateMessage {
  type: "state";
  // null when no session is running.
  set: SetView | null;
  // null until the first set starts, and again after the session times out.
  session: SessionView | null;
  devices: DeviceRow[];
}
