import { describe, expect, it, vi } from "vitest";
import {
  chooseRenderer,
  contextLostHandler,
  frameloopFor,
  useHologramPref,
} from "./renderer";

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

describe("contextLostHandler", () => {
  it("ignores the loss R3F forces after the canvas is removed (switching to 2D)", () => {
    const canvas = document.createElement("canvas");
    const onLost = vi.fn();
    contextLostHandler(canvas, onLost)(new Event("webglcontextlost"));
    expect(onLost).not.toHaveBeenCalled();
  });
  it("reports a real loss while the canvas is on screen", () => {
    const canvas = document.createElement("canvas");
    document.body.appendChild(canvas);
    const onLost = vi.fn();
    const e = new Event("webglcontextlost", { cancelable: true });
    contextLostHandler(canvas, onLost)(e);
    expect(onLost).toHaveBeenCalledTimes(1);
    expect(e.defaultPrevented).toBe(true);
    canvas.remove();
  });
});
