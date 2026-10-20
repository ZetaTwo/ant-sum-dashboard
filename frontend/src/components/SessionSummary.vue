<script setup lang="ts">
import type { SessionView } from "../api/types";
import { formatDistance, formatDuration } from "../format";

defineProps<{ session: SessionView | null; now: number }>();
</script>

<template>
  <section class="session">
    <span class="label muted">Session</span>
    <span class="distance">{{ formatDistance(session?.distance_m ?? 0) }}</span>
    <span v-if="session" class="duration muted">
      {{ formatDuration(now - session.started_at_ms) }}
    </span>
  </section>
</template>

<style scoped>
.session {
  display: flex;
  justify-content: center;
  align-items: baseline;
  gap: 0.75rem;
  margin: 1.5rem 0 2rem;
  font-size: 1.25rem;
}

.duration {
  font-size: 1rem;
}

.label {
  font-size: 0.85rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
</style>
