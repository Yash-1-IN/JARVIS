import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AssistantPhase } from "../../types/generated/AssistantPhase";
import type { Snapshot } from "../../types/generated/Snapshot";
import type { TurnInterrupted } from "../../types/generated/TurnInterrupted";
import type { TurnStreamEvent } from "../../types/generated/TurnStreamEvent";
import { MOCK_FAULTS } from "./faults";
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

describe("MockTransport submitText", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("streams deltas then exactly one done, and drives the snapshot", async () => {
    const t = new MockTransport();
    const snaps = record(t);
    const events: TurnStreamEvent[] = [];
    const id = await t.submitText("hello", (e) => events.push(e));
    expect(snaps[snaps.length - 1]).toMatchObject({
      phase: "thinking",
      activeRequestId: id,
      cloudInUse: true,
    });

    vi.advanceTimersByTime(60_000);
    const text = events
      .filter((e) => e.type === "text_delta")
      .map((e) => (e.type === "text_delta" ? e.text : ""))
      .join("");
    expect(text).toContain("MOCK");
    expect(text).toContain('"hello"');
    expect(events.filter((e) => e.type === "done")).toEqual([
      { type: "done", stopReason: "end" },
    ]);
    expect(events[events.length - 1].type).toBe("done");
    expect(snaps[snaps.length - 1]).toMatchObject({
      phase: "idle",
      activeRequestId: null,
      cloudInUse: false,
    });
  });

  it("cancel ends the stream with done cancelled and no more deltas", async () => {
    const t = new MockTransport();
    const events: TurnStreamEvent[] = [];
    const id = await t.submitText("hello there", (e) => events.push(e));
    vi.advanceTimersByTime(500);
    await t.cancel(id);
    const count = events.length;
    vi.advanceTimersByTime(60_000);
    expect(events).toHaveLength(count);
    expect(events[count - 1]).toEqual({
      type: "done",
      stopReason: "cancelled",
    });
  });
});

describe("MockTransport fault injection", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it.each(Object.entries(MOCK_FAULTS))(
    "%s ends the turn with a typed error",
    async (name, err) => {
      const t = new MockTransport();
      const snaps = record(t);
      const events: TurnStreamEvent[] = [];
      t.failNextTurn(name as keyof typeof MOCK_FAULTS);
      const id = await t.submitText("hi", (e) => events.push(e));
      vi.advanceTimersByTime(60_000);

      expect(events.filter((e) => e.type === "text_delta")).toHaveLength(0);
      expect(events).toEqual([
        {
          type: "done",
          stopReason: err.code === "SPEND_CAP_REACHED" ? "spend_cap" : "error",
        },
      ]);
      expect(snaps[snaps.length - 1]).toMatchObject({
        phase: "error",
        activeRequestId: null,
        lastError: { code: err.code, retryable: err.retryable, requestId: id },
      });
    },
  );

  it("a fault applies to one turn only", async () => {
    const t = new MockTransport();
    t.failNextTurn("http-500");
    await t.submitText("a", () => {});
    vi.advanceTimersByTime(60_000);
    const events: TurnStreamEvent[] = [];
    await t.submitText("b", (e) => events.push(e));
    vi.advanceTimersByTime(60_000);
    expect(events[events.length - 1]).toEqual({
      type: "done",
      stopReason: "end",
    });
    expect((await t.getSnapshot()).lastError).toBeNull();
  });
});

describe("MockTransport settings", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("stores only a hint for a valid key, rejects bad keys and offline checks", async () => {
    const t = new MockTransport();
    const s = await t.setApiKey("  sk-ant-mock-0123456789abWXYZ \n");
    expect(s).toEqual({ isSet: true, storage: "keychain", hint: "…WXYZ" });
    expect(JSON.stringify(await t.getSettings())).not.toContain("sk-ant");
    expect((await t.setApiKey("short-key-ok")).hint).toBeNull();

    await expect(t.setApiKey("bad-key-0123456789")).rejects.toMatchObject({
      code: "PROVIDER_AUTH",
    });
    await expect(t.setApiKey("has space")).rejects.toMatchObject({
      code: "PROVIDER_AUTH",
    });
    t.runScript("offline");
    vi.advanceTimersByTime(1000);
    await expect(t.setApiKey("sk-ant-mock-0123456789")).rejects.toMatchObject({
      code: "NETWORK_OFFLINE",
    });
    expect((await t.clearApiKey()).isSet).toBe(false);
  });

  it("applies a patch: omitted fields unchanged, null clears, unknown model rejected", async () => {
    const t = new MockTransport();
    let s = await t.updateSettings({
      dailyCapMicroUsd: 2_000_000,
      monthlyCapMicroUsd: 9_000_000,
    });
    expect([s.dailyCapMicroUsd, s.monthlyCapMicroUsd]).toEqual([
      2_000_000, 9_000_000,
    ]);
    s = await t.updateSettings({ dailyCapMicroUsd: null });
    expect([s.dailyCapMicroUsd, s.monthlyCapMicroUsd]).toEqual([
      null,
      9_000_000,
    ]);
    await expect(t.updateSettings({ model: "gpt-nope" })).rejects.toMatchObject(
      { code: "INTERNAL" },
    );
    await expect(
      t.updateSettings({ dailyCapMicroUsd: -1 }),
    ).rejects.toMatchObject({ code: "INTERNAL" });
  });

  it("records usage per reply, emits usage.updated, and blocks at the cap", async () => {
    const t = new MockTransport();
    const seen: number[] = [];
    t.onUsage((u) => seen.push(u.todayMicroUsd));
    await t.updateSettings({ dailyCapMicroUsd: 1_500 });
    await t.submitText("one", () => {});
    vi.advanceTimersByTime(60_000);
    expect((await t.getUsage()).todayMicroUsd).toBe(1_500);
    expect(seen[seen.length - 1]).toBe(1_500);

    const events: TurnStreamEvent[] = [];
    await t.submitText("two", (e) => events.push(e));
    vi.advanceTimersByTime(60_000);
    expect(events).toEqual([{ type: "done", stopReason: "spend_cap" }]);
    expect((await t.getSnapshot()).lastError?.code).toBe("SPEND_CAP_REACHED");
  });

  it("reports the next UTC day and month boundaries", async () => {
    vi.setSystemTime(Date.UTC(2026, 10, 15, 13, 0));
    const u = await new MockTransport().getUsage();
    expect(u.dayResetsAt).toBe(Date.UTC(2026, 10, 16));
    expect(u.monthResetsAt).toBe(Date.UTC(2026, 11, 1));
  });
});

describe("MockTransport interrupts", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("cancel broadcasts user_cancel; a wake word returns to listening", async () => {
    const t = new MockTransport();
    const seen: TurnInterrupted[] = [];
    t.onInterrupted((e) => seen.push(e));
    const events: TurnStreamEvent[] = [];

    const a = await t.submitText("one", (e) => events.push(e));
    vi.advanceTimersByTime(500);
    await t.cancel(a);
    expect(seen).toEqual([{ requestId: a, reason: "user_cancel" }]);
    expect((await t.getSnapshot()).phase).toBe("idle");

    const b = await t.submitText("two", (e) => events.push(e));
    vi.advanceTimersByTime(500);
    t.interrupt("wake_word");
    expect(seen[1]).toEqual({ requestId: b, reason: "wake_word" });
    expect(await t.getSnapshot()).toMatchObject({
      phase: "listening",
      mic: "capturing",
    });
    expect(events.filter((e) => e.type === "done")).toEqual([
      { type: "done", stopReason: "cancelled" },
      { type: "done", stopReason: "cancelled" },
    ]);

    t.interrupt("hotkey"); // nothing active: no event
    expect(seen).toHaveLength(2);
  });
});
