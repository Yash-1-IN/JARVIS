import { describe, expect, it } from "vitest";
import { chooseRenderer, frameloopFor, useHologramPref } from "./renderer";

describe("chooseRenderer (ADR-015)", () => {
  it("is 2D without WebGL, whatever the choice", () => {
    for (const pref of ["auto", "3d", "2d"] as const) {
      expect(chooseRenderer(pref, { webgl: false, linux: false })).toBe("2d");
    }
  });
  it("follows an explicit choice", () => {
    expect(chooseRenderer("2d", { webgl: true, linux: false })).toBe("2d");
    expect(chooseRenderer("3d", { webgl: true, linux: true })).toBe("3d");
  });
  it("Automatic is 3D, except 2D on Linux (D-08)", () => {
    expect(chooseRenderer("auto", { webgl: true, linux: false })).toBe("3d");
    expect(chooseRenderer("auto", { webgl: true, linux: true })).toBe("2d");
  });
});

describe("frameloopFor (spec §10: 0 frames idle or hidden)", () => {
  it("never renders while hidden", () => {
    expect(frameloopFor("pulse", true, false)).toBe("never");
    expect(frameloopFor("none", true, false)).toBe("never");
  });
  it("renders on demand when idle or with reduced motion", () => {
    expect(frameloopFor("none", false, false)).toBe("demand");
    expect(frameloopFor("spin", false, true)).toBe("demand");
  });
  it("animates only while something moves", () => {
    for (const m of ["pulse", "breathe", "spin", "wave"] as const) {
      expect(frameloopFor(m, false, false)).toBe("always");
    }
  });
});

describe("useHologramPref", () => {
  it("keeps the choice in local storage", () => {
    useHologramPref.getState().setPref("2d");
    expect(localStorage.getItem("jarvis.hologram")).toBe("2d");
    expect(useHologramPref.getState().pref).toBe("2d");
    useHologramPref.getState().setPref("auto");
  });
});
