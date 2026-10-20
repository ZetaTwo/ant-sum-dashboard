import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import DeviceTable from "./DeviceTable.vue";
import SessionSummary from "./SessionSummary.vue";
import SetSummary from "./SetSummary.vue";
import type { DeviceRow, SetView } from "../api/types";

const NOW = 1_000_000;
const secondsAgo = (s: number) => NOW - s * 1000;

const set = (over: Partial<SetView> = {}): SetView => ({
  distance_m: 0,
  started_at_ms: NOW,
  ended_at_ms: null,
  ...over,
});

const device = (over: Partial<DeviceRow> = {}): DeviceRow => ({
  device_id: 1,
  device_type: "rower",
  set_distance_m: 0,
  session_distance_m: 0,
  last_seen_ms: NOW,
  ...over,
});

describe("SetSummary", () => {
  it("waits for data when there's no set", () => {
    const wrapper = mount(SetSummary, { props: { set: null, now: NOW } });
    expect(wrapper.text()).toContain("0 m");
    expect(wrapper.text()).toContain("waiting for data…");
  });

  it("shows a running set's distance and time so far", () => {
    const wrapper = mount(SetSummary, {
      props: { set: set({ distance_m: 1234, started_at_ms: secondsAgo(75) }), now: NOW },
    });
    expect(wrapper.text()).toContain("1.23 km");
    expect(wrapper.text()).toContain("set · 1:15");
  });

  it("keeps a finished set's numbers and counts the rest", () => {
    const wrapper = mount(SetSummary, {
      props: {
        set: set({ distance_m: 501, started_at_ms: secondsAgo(100), ended_at_ms: secondsAgo(20) }),
        now: NOW,
      },
    });
    expect(wrapper.text()).toContain("501 m");
    expect(wrapper.text()).toContain("last set 1:20 · resting 0:20");
  });
});

describe("SessionSummary", () => {
  it("shows the session's distance and duration", () => {
    const wrapper = mount(SessionSummary, {
      props: { session: { distance_m: 2500, started_at_ms: secondsAgo(600) }, now: NOW },
    });
    expect(wrapper.text()).toContain("2.50 km");
    expect(wrapper.text()).toContain("10:00");
  });

  it("shows zero and no duration without a session", () => {
    const wrapper = mount(SessionSummary, { props: { session: null, now: NOW } });
    expect(wrapper.text()).toContain("0 m");
    expect(wrapper.find(".duration").exists()).toBe(false);
  });
});

describe("DeviceTable", () => {
  it("lists devices by id with their type and both distances", () => {
    const wrapper = mount(DeviceTable, {
      props: {
        devices: [
          device({ device_id: 9, device_type: "nordic_skier", set_distance_m: 120 }),
          device({ device_id: 3, device_type: "bike", set_distance_m: 50, session_distance_m: 1500 }),
        ],
        now: NOW,
      },
    });
    const rows = wrapper.findAll("tbody tr").map((tr) => tr.findAll("td").map((td) => td.text()));
    expect(rows).toEqual([
      ["3", "Bike", "50 m", "1.50 km"],
      ["9", "Ski erg", "120 m", "0 m"],
    ]);
  });

  it("dims devices that haven't reported for over five seconds", () => {
    const wrapper = mount(DeviceTable, {
      props: {
        devices: [
          device({ device_id: 1, last_seen_ms: secondsAgo(5) }),
          device({ device_id: 2, last_seen_ms: secondsAgo(6) }),
        ],
        now: NOW,
      },
    });
    const stale = wrapper.findAll("tbody tr").map((tr) => tr.classes("stale"));
    expect(stale).toEqual([false, true]);
  });
});
