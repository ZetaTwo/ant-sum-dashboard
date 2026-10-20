<script setup lang="ts">
import { computed } from "vue";
import type { SetView } from "../api/types";
import { formatDistance, formatDuration } from "../format";

const props = defineProps<{ set: SetView | null; now: number }>();

const info = computed(() => {
  const set = props.set;
  if (set === null) return "waiting for data…";
  if (set.ended_at_ms === null) {
    return `set · ${formatDuration(props.now - set.started_at_ms)}`;
  }
  const length = formatDuration(set.ended_at_ms - set.started_at_ms);
  return `last set ${length} · resting ${formatDuration(props.now - set.ended_at_ms)}`;
});
</script>

<template>
  <section>
    <div class="distance">{{ formatDistance(set?.distance_m ?? 0) }}</div>
    <div class="info muted">{{ info }}</div>
  </section>
</template>

<style scoped>
.distance {
  font-size: 4rem;
  font-weight: 700;
  text-align: center;
  line-height: 1;
}

.info {
  text-align: center;
  margin-top: 0.5rem;
}
</style>
