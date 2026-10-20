import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { connect, RECONNECT_DELAY_MS, STALL_AFTER_MS } from "./ws";
import type { StateMessage } from "./types";

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  url: string;
  closed = false;
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }

  close() {
    this.closed = true;
  }

  receive(msg: StateMessage) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
}

const STATE: StateMessage = { type: "state", set: null, session: null, devices: [] };

let messages: StateMessage[];
let changes: boolean[];
let disconnect: () => void;

const latest = () => FakeWebSocket.instances.at(-1)!;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeWebSocket);
  FakeWebSocket.instances = [];
  messages = [];
  changes = [];
  disconnect = connect({
    onMessage: (msg) => messages.push(msg),
    onConnectionChange: (connected) => changes.push(connected),
  });
});

afterEach(() => {
  disconnect();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("connects to /ws on the page's host and passes messages on", () => {
  expect(latest().url).toBe(`ws://${location.host}/ws`);
  latest().onopen?.();
  latest().receive(STATE);
  expect(changes).toEqual([true]);
  expect(messages).toEqual([STATE]);
});

it("reconnects after a delay when the connection closes", () => {
  latest().onopen?.();
  latest().onclose?.();
  expect(changes).toEqual([true, false]);
  expect(FakeWebSocket.instances).toHaveLength(1);
  vi.advanceTimersByTime(RECONNECT_DELAY_MS);
  expect(FakeWebSocket.instances).toHaveLength(2);
});

it("reconnects only once when an error is followed by a close", () => {
  latest().onerror?.();
  latest().onclose?.();
  expect(changes).toEqual([false]);
  vi.advanceTimersByTime(RECONNECT_DELAY_MS * 5);
  expect(FakeWebSocket.instances).toHaveLength(2);
});

it("replaces a connection that goes silent without closing", () => {
  const first = latest();
  first.onopen?.();
  vi.advanceTimersByTime(STALL_AFTER_MS);
  expect(first.closed).toBe(true);
  expect(changes).toEqual([true, false]);
  vi.advanceTimersByTime(RECONNECT_DELAY_MS);
  expect(latest()).not.toBe(first);
});

it("keeps a connection that keeps receiving messages", () => {
  latest().onopen?.();
  for (let i = 0; i < 10; i++) {
    vi.advanceTimersByTime(STALL_AFTER_MS - 1);
    latest().receive(STATE);
  }
  expect(FakeWebSocket.instances).toHaveLength(1);
  expect(latest().closed).toBe(false);
});

it("stays closed once disconnected", () => {
  latest().onopen?.();
  disconnect();
  expect(latest().closed).toBe(true);
  latest().onclose?.();
  vi.advanceTimersByTime(STALL_AFTER_MS + RECONNECT_DELAY_MS);
  expect(FakeWebSocket.instances).toHaveLength(1);
  expect(changes).toEqual([true]);
});

it("cancels a pending reconnect when disconnected", () => {
  latest().onclose?.();
  disconnect();
  vi.advanceTimersByTime(RECONNECT_DELAY_MS);
  expect(FakeWebSocket.instances).toHaveLength(1);
});
