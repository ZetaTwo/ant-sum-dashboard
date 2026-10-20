import type { StateMessage } from "./types";

// The backend publishes state at least once a second, so silence for this
// long means the connection is dead even if the socket hasn't noticed.
export const STALL_AFTER_MS = 5000;
export const RECONNECT_DELAY_MS = 1000;

export interface WsCallbacks {
  onMessage: (msg: StateMessage) => void;
  onConnectionChange: (connected: boolean) => void;
}

function wsUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}

/**
 * Keeps a connection to the backend's state stream open, reconnecting after
 * it drops or stalls. Returns a function that closes it for good.
 */
export function connect(callbacks: WsCallbacks): () => void {
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let closeCurrent = () => false;

  function open() {
    const ws = new WebSocket(wsUrl());
    let stallTimer: ReturnType<typeof setTimeout> | undefined;
    let closed = false;

    const close = () => {
      if (closed) return false;
      closed = true;
      clearTimeout(stallTimer);
      ws.close();
      return true;
    };
    // Reconnects without waiting for `onclose`: on a half-dead connection
    // the closing handshake can take minutes to time out.
    const reconnect = () => {
      if (!close()) return;
      callbacks.onConnectionChange(false);
      retryTimer = setTimeout(open, RECONNECT_DELAY_MS);
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
    closeCurrent = close;
    // Also covers a connection attempt that hangs.
    armStallTimer();
  }

  open();
  return () => {
    clearTimeout(retryTimer);
    closeCurrent();
  };
}
