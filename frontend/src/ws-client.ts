import type { StateMessage } from "./types";

export interface WsCallbacks {
  onMessage: (msg: StateMessage) => void;
  onConnectionChange: (connected: boolean) => void;
}

function wsUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}

export function connect(callbacks: WsCallbacks): void {
  let ws: WebSocket;

  function open() {
    ws = new WebSocket(wsUrl());
    ws.onopen = () => callbacks.onConnectionChange(true);
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data) as StateMessage;
      callbacks.onMessage(msg);
    };
    ws.onclose = () => {
      callbacks.onConnectionChange(false);
      setTimeout(open, 1000);
    };
    ws.onerror = () => ws.close();
  }

  open();
}
