import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { WsCallbacks } from "../api/ws";
import type { StateMessage } from "../api/types";

vi.mock("../api/ws", () => ({ connect: vi.fn() }));
const { connect } = await import("../api/ws");
const { useDashboardStore } = await import("./dashboard");

const STATE: StateMessage = { type: "state", set: null, session: null, devices: [] };

let callbacks: WsCallbacks;
const disconnect = vi.fn();

beforeEach(() => {
  vi.useFakeTimers();
  setActivePinia(createPinia());
  vi.resetAllMocks();
  vi.mocked(connect).mockImplementation((cbs) => {
    callbacks = cbs;
    return disconnect;
  });
});

afterEach(() => vi.useRealTimers());

it("tracks the connection and keeps the latest state", () => {
  const store = useDashboardStore();
  store.start();
  expect(store.connection).toBe("connecting");
  callbacks.onConnectionChange(true);
  expect(store.connection).toBe("connected");
  callbacks.onMessage(STATE);
  expect(store.state).toEqual(STATE);
  callbacks.onConnectionChange(false);
  expect(store.connection).toBe("reconnecting");
  // The last known state stays on screen while reconnecting.
  expect(store.state).toEqual(STATE);
});

it("connects only once however often it's started", () => {
  const store = useDashboardStore();
  store.start();
  store.start();
  expect(connect).toHaveBeenCalledTimes(1);
});

it("ticks its clock every second until stopped", () => {
  const store = useDashboardStore();
  store.start();
  const before = store.now;
  vi.advanceTimersByTime(3000);
  expect(store.now).toBe(before + 3000);
  store.stop();
  vi.advanceTimersByTime(3000);
  expect(store.now).toBe(before + 3000);
});

it("disconnects when stopped and can start again", () => {
  const store = useDashboardStore();
  store.start();
  store.stop();
  expect(disconnect).toHaveBeenCalledTimes(1);
  store.start();
  expect(connect).toHaveBeenCalledTimes(2);
});
