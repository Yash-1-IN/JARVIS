import { describe, expect, it } from "vitest";
import { ease, energyFor, FLARE_SECONDS, flareAt, sparkle } from "./motion";

const samples = (m: Parameters<typeof energyFor>[0]) =>
  Array.from({ length: 600 }, (_, i) => energyFor(m, i / 60));

describe("energyFor", () => {
  it("stays in 0..1 for every state at every moment", () => {
    for (const m of ["none", "pulse", "spin", "breathe", "wave"] as const) {
      for (const e of samples(m)) {
        expect(e).toBeGreaterThanOrEqual(0);
        expect(e).toBeLessThanOrEqual(1);
      }
    }
  });
  it("idle is exactly still", () => {
    expect(samples("none").every((e) => e === 0)).toBe(true);
  });
  it("speaking jolts: wide swings, hard kicks, and it does not repeat in a short loop", () => {
    const e = samples("wave");
    expect(Math.max(...e) - Math.min(...e)).toBeGreaterThan(0.6);
    // Frame-to-frame jumps exist (a kick), unlike the calm listening beat.
    const jump = (xs: number[]) =>
      Math.max(...xs.slice(1).map((x, i) => Math.abs(x - xs[i])));
    expect(jump(e)).toBeGreaterThan(0.1);
    expect(jump(samples("pulse"))).toBeLessThan(0.1);
    expect(new Set(e.map((x) => x.toFixed(3))).size).toBeGreaterThan(300);
  });
  it("speaking is livelier on average than listening", () => {
    const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
    expect(mean(samples("wave"))).toBeGreaterThan(mean(samples("pulse")));
  });
  it("is deterministic", () => {
    expect(energyFor("wave", 3.14)).toBe(energyFor("wave", 3.14));
  });
});

describe("sparkle", () => {
  it("is in 0..1 and differs between sparks", () => {
    for (let s = 0; s < 50; s++) {
      const v = sparkle(s, 1.23);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    }
    expect(sparkle(1, 2)).not.toBe(sparkle(2, 2));
  });
});

describe("ease", () => {
  it("closes the gap smoothly and never overshoots", () => {
    let v = 0;
    let last = 0;
    for (let i = 0; i < 120; i++) {
      v = ease(v, 1, 1 / 60);
      expect(v).toBeGreaterThanOrEqual(last);
      expect(v).toBeLessThanOrEqual(1);
      last = v;
    }
    expect(v).toBeGreaterThan(0.99);
  });
  it("does not jump on the first frame, and works downward too", () => {
    expect(ease(0, 1, 1 / 60)).toBeLessThan(0.2);
    expect(ease(1, 0, 1 / 60)).toBeGreaterThan(0.8);
  });
  it("is frame-rate independent and a zero or negative step changes nothing", () => {
    let a = 0;
    for (let i = 0; i < 60; i++) a = ease(a, 1, 1 / 60);
    let b = 0;
    for (let i = 0; i < 30; i++) b = ease(b, 1, 1 / 30);
    expect(a).toBeCloseTo(b, 6);
    expect(ease(0.4, 1, 0)).toBe(0.4);
    expect(ease(0.4, 1, -1)).toBe(0.4);
  });
});

describe("flareAt", () => {
  it("is full at the change, fades monotonically, and is gone after FLARE_SECONDS", () => {
    expect(flareAt(0)).toBe(1);
    let last = 1;
    for (let i = 1; i <= 100; i++) {
      const f = flareAt(i / 50);
      expect(f).toBeLessThan(last);
      expect(f).toBeGreaterThan(0);
      last = f;
    }
    expect(flareAt(FLARE_SECONDS)).toBeLessThanOrEqual(0.0100001);
    expect(FLARE_SECONDS).toBeGreaterThan(0.5);
    expect(FLARE_SECONDS).toBeLessThan(2);
  });
  it("is nothing before a change", () => {
    expect(flareAt(-1)).toBe(0);
    expect(flareAt(Number.NEGATIVE_INFINITY)).toBe(0);
  });
});
