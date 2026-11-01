import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

type Handler = (e: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const unlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, h: Handler) => {
    handlers.set(name, h);
    return unlisten;
  },
}));

import { PROTOCOL_VERSION } from "../protocol";
import { SNAPSHOT_EVENT, TauriTransport } from "./tauri";

describe("TauriTransport", () => {
  beforeEach(() => invoke.mockReset());

  it("ping calls the ping command and returns its payload", async () => {
    invoke.mockResolvedValue({
      pong: true,
      protocolVersion: PROTOCOL_VERSION,
      appVersion: "0.1.0",
    });
    const res = await new TauriTransport().ping();
    expect(invoke).toHaveBeenCalledWith("ping");
    expect(res.pong).toBe(true);
    expect(res.protocolVersion).toBe(PROTOCOL_VERSION);
  });

  it("getSnapshot calls get_snapshot", async () => {
    invoke.mockResolvedValue({ seq: 0, mic: "off", phase: "idle" });
    const snap = await new TauriTransport().getSnapshot();
    expect(invoke).toHaveBeenCalledWith("get_snapshot");
    expect(snap.phase).toBe("idle");
  });

  it("delivers state_snapshot envelopes of our protocol version", async () => {
    const got: unknown[] = [];
    const stop = new TauriTransport().onSnapshot((s) => got.push(s));
    await Promise.resolve();
    const emit = handlers.get(SNAPSHOT_EVENT)!;
    const env = (v: number, kind: string, seq: number) => ({
      payload: { v, seq, id: "x", ts: 0, kind, payload: { seq, mic: "muted" } },
    });
    emit(env(PROTOCOL_VERSION, "state.snapshot", 1));
    emit(env(PROTOCOL_VERSION + 1, "state.snapshot", 2));
    emit(env(PROTOCOL_VERSION, "error.raised", 3));
    expect(got).toEqual([{ seq: 1, mic: "muted" }]);

    stop();
    stop();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("maps settings methods to the contract command names", async () => {
    const t = new TauriTransport();
    invoke.mockResolvedValue({});
    await t.getSettings();
    await t.updateSettings({ dailyCapMicroUsd: null });
    await t.setApiKey("k");
    await t.clearApiKey();
    await t.getUsage();
    expect(invoke.mock.calls).toEqual([
      ["settings_get"],
      ["settings_set", { patch: { dailyCapMicroUsd: null } }],
      ["api_key_set", { key: "k" }],
      ["api_key_clear"],
      ["usage_get"],
    ]);
    await t.appsInstalled();
    expect(invoke).toHaveBeenLastCalledWith("apps_installed");
  });

  it("delivers usage_updated envelopes", async () => {
    const got: unknown[] = [];
    new TauriTransport().onUsage((u) => got.push(u));
    await Promise.resolve();
    handlers.get("usage_updated")!({
      payload: {
        v: PROTOCOL_VERSION,
        seq: 1,
        id: "x",
        ts: 0,
        kind: "usage.updated",
        payload: { todayMicroUsd: 7 },
      },
    });
    expect(got).toEqual([{ todayMicroUsd: 7 }]);
  });

  it("delivers turn_interrupted envelopes", async () => {
    const got: unknown[] = [];
    new TauriTransport().onInterrupted((e) => got.push(e));
    await Promise.resolve();
    handlers.get("turn_interrupted")!({
      payload: {
        v: PROTOCOL_VERSION,
        seq: 2,
        id: "x",
        ts: 0,
        kind: "turn.interrupted",
        payload: { requestId: "r", reason: "hotkey" },
      },
    });
    expect(got).toEqual([{ requestId: "r", reason: "hotkey" }]);
  });

  it("sends resolve_approval with the decision object", async () => {
    invoke.mockResolvedValue(undefined);
    const d = {
      approvalId: "a",
      argsDigest: "d",
      decision: "approve" as const,
    };
    await new TauriTransport().resolveApproval(d);
    expect(invoke).toHaveBeenCalledWith("resolve_approval", { decision: d });
  });

  it("maps the activity log to activity_list and activity_appended", async () => {
    invoke.mockResolvedValue([]);
    const t = new TauriTransport();
    await t.listActivity(50, "abc");
    expect(invoke).toHaveBeenLastCalledWith("activity_list", {
      limit: 50,
      before: "abc",
    });
    const got: unknown[] = [];
    t.onActivity((e) => got.push(e));
    await Promise.resolve();
    handlers.get("activity_appended")!({
      payload: {
        v: PROTOCOL_VERSION,
        seq: 3,
        id: "x",
        ts: 0,
        kind: "activity.appended",
        payload: { id: "a1" },
      },
    });
    expect(got).toEqual([{ id: "a1" }]);
  });

  it("maps memory to the memory_* commands and memory_proposed", async () => {
    invoke.mockResolvedValue([]);
    const t = new TauriTransport();
    await t.listMemories("proposed");
    expect(invoke).toHaveBeenLastCalledWith("memory_list", {
      status: "proposed",
    });
    await t.listMemories();
    expect(invoke).toHaveBeenLastCalledWith("memory_list", {
      status: undefined,
    });
    await t.acceptMemory("m1");
    expect(invoke).toHaveBeenLastCalledWith("memory_accept", { id: "m1" });
    await t.editMemory("m1", "Likes tea");
    expect(invoke).toHaveBeenLastCalledWith("memory_edit", {
      id: "m1",
      content: "Likes tea",
    });
    await t.deleteMemory("m1");
    expect(invoke).toHaveBeenLastCalledWith("memory_delete", { id: "m1" });

    const got: unknown[] = [];
    t.onMemoryProposed((m) => got.push(m));
    await Promise.resolve();
    const envelope = (kind: string) => ({
      payload: {
        v: PROTOCOL_VERSION,
        seq: 4,
        id: "x",
        ts: 0,
        kind,
        payload: { id: "m2" },
      },
    });
    handlers.get("memory_proposed")!(envelope("memory.proposed"));
    // Another kind on the same event name is dropped.
    handlers.get("memory_proposed")!(envelope("usage.updated"));
    expect(got).toEqual([{ id: "m2" }]);
  });

  it("maps history and export to their commands", async () => {
    invoke.mockResolvedValue([]);
    const t = new TauriTransport();
    await t.listConversations(30, "c1");
    expect(invoke).toHaveBeenLastCalledWith("history_list", {
      limit: 30,
      before: "c1",
    });
    await t.getConversation("c2");
    expect(invoke).toHaveBeenLastCalledWith("history_get", {
      conversationId: "c2",
    });
    await t.deleteAllHistory();
    expect(invoke).toHaveBeenLastCalledWith("history_delete_all");
    await t.getHistorySettings();
    expect(invoke).toHaveBeenLastCalledWith("history_settings_get");
    await t.setHistorySettings({ retention: "days7" });
    expect(invoke).toHaveBeenLastCalledWith("history_settings_set", {
      settings: { retention: "days7" },
    });
    await t.exportData();
    expect(invoke).toHaveBeenLastCalledWith("memory_export");
  });
});
