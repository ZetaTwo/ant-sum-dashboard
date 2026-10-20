<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import DeviceTable from "./components/DeviceTable.vue";
import SessionSummary from "./components/SessionSummary.vue";
import SetSummary from "./components/SetSummary.vue";
import { useDashboardStore } from "./stores/dashboard";

const dashboard = useDashboardStore();

const statusLabels = {
  connecting: "connecting…",
  connected: "connected",
  reconnecting: "reconnecting…",
};

onMounted(dashboard.start);
onUnmounted(dashboard.stop);
</script>

<template>
  <main class="app-shell">
    <div class="status" :class="{ 'status--connected': dashboard.connection === 'connected' }">
      {{ statusLabels[dashboard.connection] }}
    </div>
    <SetSummary :set="dashboard.state?.set ?? null" :now="dashboard.now" />
    <SessionSummary :session="dashboard.state?.session ?? null" :now="dashboard.now" />
    <DeviceTable :devices="dashboard.state?.devices ?? []" :now="dashboard.now" />
  </main>
</template>

<style scoped>
.status {
  font-size: 0.85rem;
  text-align: right;
  margin-bottom: 1rem;
  color: var(--color-muted);
}

.status--connected {
  color: var(--color-accent);
}
</style>
