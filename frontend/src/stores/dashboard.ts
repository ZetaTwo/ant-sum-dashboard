import { ref } from "vue";
import { defineStore } from "pinia";
import { connect } from "../api/ws";
import type { StateMessage } from "../api/types";

export type Connection = "connecting" | "connected" | "reconnecting";

export const useDashboardStore = defineStore("dashboard", () => {
  const state = ref<StateMessage | null>(null);
  const connection = ref<Connection>("connecting");
  /** Wall clock, ticking every second so durations and staleness stay
   * current between messages. */
  const now = ref(Date.now());
  let teardown: (() => void) | null = null;

  function start() {
    if (teardown) return;
    const disconnect = connect({
      onMessage: (msg) => {
        state.value = msg;
      },
      onConnectionChange: (connected) => {
        connection.value = connected ? "connected" : "reconnecting";
      },
    });
    const clock = setInterval(() => {
      now.value = Date.now();
    }, 1000);
    teardown = () => {
      disconnect();
      clearInterval(clock);
    };
  }

  function stop() {
    teardown?.();
    teardown = null;
  }

  return { state, connection, now, start, stop };
});
