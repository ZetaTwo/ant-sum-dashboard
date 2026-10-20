import type { StateMessage } from "./types";

// The backend publishes state at least once a second, so silence for this
// long means the connection is dead even if the socket hasn't noticed.
const STALL_AFTER_MS = 5000;

export interface WsCallbacks {
  onMessage: (msg: StateMessage) => void;
  onConnectionChange: (connected: boolean) => void;
}

function wsUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}

export function connect(callbacks: WsCallbacks): void {
  function open() {
    const ws = new WebSocket(wsUrl());
    let stallTimer: ReturnType<typeof setTimeout> | undefined;
    let done = false;

    // Reconnects without waiting for `onclose`: on a half-dead connection
    // the closing handshake can take minutes to time out.
    const reconnect = () => {
      if (done) return;
      done = true;
      clearTimeout(stallTimer);
      ws.close();
      callbacks.onConnectionChange(false);
      setTimeout(open, 1000);
    };
    const armStallTimer = () => {
      clearTimeout(stallTimer);
      stallTimer = setTimeout(reconnect, STALL_AFTER_MS);
    };

    ws.onopen = () => {
      callbacks.onConnectionChange(true);
      armStallTimer();
    };
    ws.onmessage = (ev) => {
      armStallTimer();
      callbacks.onMessage(JSON.parse(ev.data) as StateMessage);
    };
    ws.onclose = reconnect;
    ws.onerror = reconnect;
  }

  open();
}
