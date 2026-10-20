import type { StateMessage, DeviceRow } from "./types";

const STALE_AFTER_MS = 5000;

const setDistanceEl = document.getElementById("set-distance")!;
const setInfoEl = document.getElementById("set-info")!;
const sessionDistanceEl = document.getElementById("session-distance")!;
const sessionInfoEl = document.getElementById("session-info")!;
const rowsEl = document.getElementById("device-rows")!;
const statusEl = document.getElementById("status")!;

let lastState: StateMessage | null = null;

const deviceTypeLabels: Record<DeviceRow["device_type"], string> = {
  bike: "Bike",
  rower: "Rower",
  nordic_skier: "Ski erg",
  other: "Other",
};

function formatDistance(m: number): string {
  if (m >= 1000) return `${(m / 1000).toFixed(2)} km`;
  return `${m.toFixed(0)} m`;
}

function formatDuration(ms: number): string {
  const totalSecs = Math.max(0, Math.floor(ms / 1000));
  const mins = Math.floor(totalSecs / 60);
  const secs = String(totalSecs % 60).padStart(2, "0");
  return `${mins}:${secs}`;
}

export function setConnectionStatus(connected: boolean): void {
  statusEl.textContent = connected ? "connected" : "reconnecting…";
  statusEl.className = `status ${connected ? "status--connected" : "status--connecting"}`;
}

export function renderState(msg: StateMessage): void {
  lastState = msg;
  render(msg);
}

function render(msg: StateMessage): void {
  const now = Date.now();
  const { set, session } = msg;

  setDistanceEl.textContent = formatDistance(set?.distance_m ?? 0);
  if (set === null) {
    setInfoEl.textContent = "waiting for data…";
  } else if (set.ended_at_ms === null) {
    setInfoEl.textContent = `set · ${formatDuration(now - set.started_at_ms)}`;
  } else {
    const setLength = formatDuration(set.ended_at_ms - set.started_at_ms);
    setInfoEl.textContent = `last set ${setLength} · resting ${formatDuration(now - set.ended_at_ms)}`;
  }

  sessionDistanceEl.textContent = formatDistance(session?.distance_m ?? 0);
  sessionInfoEl.textContent =
    session === null ? "" : formatDuration(now - session.started_at_ms);

  const rows = [...msg.devices].sort((a, b) => a.device_id - b.device_id);
  rowsEl.replaceChildren(
    ...rows.map((d) => {
      const tr = document.createElement("tr");
      tr.className = now - d.last_seen_ms > STALE_AFTER_MS ? "stale" : "";
      tr.innerHTML = `
        <td>${d.device_id}</td>
        <td>${deviceTypeLabels[d.device_type] ?? d.device_type}</td>
        <td>${formatDistance(d.set_distance_m)}</td>
        <td>${formatDistance(d.session_distance_m)}</td>
      `;
      return tr;
    }),
  );
}

// Keep durations and staleness current between messages.
setInterval(() => {
  if (lastState) render(lastState);
}, 1000);
