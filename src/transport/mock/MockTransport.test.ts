import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AssistantPhase } from "../../types/generated/AssistantPhase";
import type { Snapshot } from "../../types/generated/Snapshot";
import { MockTransport } from "./MockTransport";
import { MOCK_SCRIPTS } from "./scripts";

function record(t: MockTransport): Snapshot[] {
  const seen: Snapshot[] = [];
  t.onSnapshot((s) => seen.push(s));
  return seen;
}

function totalMs(name: string): number {
  return MOCK_SCRIPTS.find((s) => s.name === name)!.steps.reduce(
    (n, s) => n + s.delayMs,
    0,
  );
}

describe("MockTransport", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("ships every scenario M1 acceptance asks for", () => {
    expect(MOCK_SCRIPTS.map((s) => s.name).sort()).toEqual(
      [
        "approval",
        "error",
        "interrupt",
        "offline",
        "wake-listen-think-speak",
      ].sort(),
    );
  });

  it("plays wake → listen → think → speak with increasing seq", () => {
    const t = new MockTransport();
    const seen = record(t);
    t.runScript("wake-listen-think-speak");
    vi.advanceTimersByTime(totalMs("wake-listen-think-speak"));

    const phases = seen.map((s) => s.phase);
    const order: AssistantPhase[] = [
      "listening",
      "thinking",
      "speaking",
      "idle",
    ];
    let i = -1;
    for (const p of order) {
      const next = phases.indexOf(p, i + 1);
      expect(next, `${p} after index ${i}`).toBeGreaterThan(i);
      i = next;
    }
    seen.forEach(
      (s, n) => n > 0 && expect(s.seq).toBeGreaterThan(seen[n - 1].seq),
    );
  });

  it("fills a 60 s expiry for scripted approvals", () => {
    vi.setSystemTime(1_000_000);
    const t = new MockTransport();
    const seen = record(t);
    t.runScript("approval");
    vi.advanceTimersByTime(1000);
    const pending = seen.find((s) => s.phase === "awaiting_approval")!
      .pendingApprovals[0];
    expect(pending.expiresAt).toBe(1_000_000 + 1000 + 60_000);
  });

  it("starting a script cancels the one in progress", () => {
    const t = new MockTransport();
    const seen = record(t);
    t.runScript("wake-listen-think-speak");
    t.runScript("offline");
    vi.advanceTimersByTime(60_000);
    expect(seen.some((s) => s.phase === "speaking")).toBe(false);
  });

  it("stops notifying after unsubscribe", () => {
    const t = new MockTransport();
    const listener = vi.fn();
    t.onSnapshot(listener)();
    t.runScript("offline");
    vi.advanceTimersByTime(60_000);
    expect(listener).not.toHaveBeenCalled();
  });

  it("rejects unknown script names", () => {
    expect(() => new MockTransport().runScript("nope")).toThrow(
      /Unknown mock script/,
    );
  });
});

describe("MockTransport commands", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("cancel stops the running script and returns to idle", async () => {
    const t = new MockTransport();
    const seen = record(t);
    t.runScript("wake-listen-think-speak");
    vi.advanceTimersByTime(800);
    expect(seen[seen.length - 1]).toMatchObject({
      phase: "listening",
      mic: "capturing",
    });

    await t.cancel("mock-req-1");
    vi.advanceTimersByTime(60_000);
    expect(seen[seen.length - 1]).toMatchObject({
      phase: "idle",
      mic: "standby",
      activeRequestId: null,
    });
    expect(seen.some((s) => s.phase === "speaking")).toBe(false);
  });

  it("cancel ignores a request id that is not active", async () => {
    const t = new MockTransport();
    const seen = record(t);
    await t.cancel("other");
    expect(seen).toHaveLength(0);
  });

  it("setMute toggles between muted and standby", async () => {
    const t = new MockTransport();
    await t.setMute(true);
    expect((await t.getSnapshot()).mic).toBe("muted");
    await t.setMute(false);
    expect((await t.getSnapshot()).mic).toBe("standby");
  });
});
