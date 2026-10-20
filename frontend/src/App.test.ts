import { beforeEach, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { WsCallbacks } from "./api/ws";

vi.mock("./api/ws", () => ({ connect: vi.fn() }));
const { connect } = await import("./api/ws");
const { default: App } = await import("./App.vue");

let callbacks: WsCallbacks;
const disconnect = vi.fn();

beforeEach(() => {
  setActivePinia(createPinia());
  vi.resetAllMocks();
  vi.mocked(connect).mockImplementation((cbs) => {
    callbacks = cbs;
    return disconnect;
  });
});

it("connects on mount, renders pushed state, and disconnects on unmount", async () => {
  const wrapper = mount(App);
  expect(connect).toHaveBeenCalledTimes(1);
  expect(wrapper.find(".status").text()).toBe("connecting…");

  callbacks.onConnectionChange(true);
  callbacks.onMessage({
    type: "state",
    set: { distance_m: 420, started_at_ms: Date.now(), ended_at_ms: null },
    session: { distance_m: 1420, started_at_ms: Date.now() },
    devices: [
      {
        device_id: 7,
        device_type: "rower",
        set_distance_m: 420,
        session_distance_m: 1420,
        last_seen_ms: Date.now(),
      },
    ],
  });
  await flushPromises();

  expect(wrapper.find(".status").text()).toBe("connected");
  expect(wrapper.text()).toContain("420 m");
  expect(wrapper.text()).toContain("1.42 km");
  expect(wrapper.findAll("tbody tr")).toHaveLength(1);

  wrapper.unmount();
  expect(disconnect).toHaveBeenCalledTimes(1);
});
