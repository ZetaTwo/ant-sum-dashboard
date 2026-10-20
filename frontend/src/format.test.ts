import { describe, expect, it } from "vitest";
import { formatDistance, formatDuration } from "./format";

describe("formatDistance", () => {
  it("shows whole meters below a kilometer", () => {
    expect(formatDistance(0)).toBe("0 m");
    expect(formatDistance(42.4)).toBe("42 m");
    expect(formatDistance(999)).toBe("999 m");
  });

  it("switches to kilometers with two decimals", () => {
    expect(formatDistance(1000)).toBe("1.00 km");
    expect(formatDistance(12345)).toBe("12.35 km");
  });
});

describe("formatDuration", () => {
  it("formats as m:ss", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(59_999)).toBe("0:59");
    expect(formatDuration(61_000)).toBe("1:01");
    expect(formatDuration(3_600_000)).toBe("60:00");
  });

  it("clamps negative durations from clock skew to zero", () => {
    expect(formatDuration(-5000)).toBe("0:00");
  });
});
