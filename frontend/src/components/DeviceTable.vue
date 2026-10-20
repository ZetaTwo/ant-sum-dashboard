<script setup lang="ts">
import { computed } from "vue";
import type { DeviceRow } from "../api/types";
import { formatDistance } from "../format";

const STALE_AFTER_MS = 5000;

const props = defineProps<{ devices: DeviceRow[]; now: number }>();

const labels: Record<DeviceRow["device_type"], string> = {
  bike: "Bike",
  rower: "Rower",
  nordic_skier: "Ski erg",
  other: "Other",
};

const rows = computed(() => [...props.devices].sort((a, b) => a.device_id - b.device_id));
</script>

<template>
  <table>
    <thead>
      <tr>
        <th>Device</th>
        <th>Type</th>
        <th>Set</th>
        <th>Session</th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="device in rows"
        :key="device.device_id"
        :class="{ stale: now - device.last_seen_ms > STALE_AFTER_MS }"
      >
        <td>{{ device.device_id }}</td>
        <td>{{ labels[device.device_type] ?? device.device_type }}</td>
        <td>{{ formatDistance(device.set_distance_m) }}</td>
        <td>{{ formatDistance(device.session_distance_m) }}</td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
table {
  width: 100%;
  border-collapse: collapse;
}

th,
td {
  text-align: left;
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid var(--color-border);
}

th {
  color: var(--color-muted);
  font-weight: 500;
  font-size: 0.85rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

tr.stale {
  opacity: 0.4;
}
</style>
