import type { StateMessage, DeviceRow } from "./types";

const STALE_AFTER_MS = 5000;

const totalEl = document.getElementById("total")!;
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

export function setConnectionStatus(connected: boolean): void {
  statusEl.textContent = connected ? "connected" : "reconnecting…";
  statusEl.className = `status ${connected ? "status--connected" : "status--connecting"}`;
}

export function renderState(msg: StateMessage): void {
  lastState = msg;
  totalEl.textContent = formatDistance(msg.total_distance_m);

  if (msg.session_started_at_ms === null) {
    sessionInfoEl.textContent = "waiting for data…";
  } else {
    const startedAgo = Math.max(0, Date.now() - msg.session_started_at_ms);
    sessionInfoEl.textContent = `session running for ${Math.floor(startedAgo / 1000)}s`;
  }

  const rows = [...msg.devices].sort((a, b) => a.device_id - b.device_id);
  rowsEl.replaceChildren(
    ...rows.map((d) => {
      const tr = document.createElement("tr");
      tr.dataset.deviceId = String(d.device_id);
      tr.className = isStale(d) ? "stale" : "";
      tr.innerHTML = `
        <td>${d.device_id}</td>
        <td>${deviceTypeLabels[d.device_type] ?? d.device_type}</td>
        <td>${formatDistance(d.distance_m)}</td>
      `;
      return tr;
    }),
  );
}

function isStale(d: DeviceRow): boolean {
  return Date.now() - d.last_seen_ms > STALE_AFTER_MS;
}

// Re-evaluate staleness independent of new WS messages, since a device can
// go quiet without the backend sending a fresh state update.
setInterval(() => {
  if (!lastState) return;
  for (const tr of rowsEl.querySelectorAll<HTMLTableRowElement>("tr[data-device-id]")) {
    const device = lastState.devices.find((d) => String(d.device_id) === tr.dataset.deviceId);
    if (device) tr.className = isStale(device) ? "stale" : "";
  }
}, 1000);
