import { beforeEach, describe, expect, it } from "vitest";
import { useChatStore } from "./chatStore";

describe("chatStore", () => {
  beforeEach(() => useChatStore.setState({ turns: [], pendingInterrupts: {} }));

  it("accumulates text deltas and stops at done", () => {
    const s = useChatStore.getState();
    s.start("k", "hi");
    s.apply("k", { type: "text_delta", text: "Hel" });
    s.apply("k", { type: "text_delta", text: "lo" });
    s.apply("k", { type: "done", stopReason: "end" });
    s.apply("k", { type: "text_delta", text: " late" });
    expect(useChatStore.getState().turns[0]).toMatchObject({
      reply: "Hello",
      stopReason: "end",
    });
  });

  it("accepts events before the requestId arrives", () => {
    const s = useChatStore.getState();
    s.start("k", "hi");
    s.apply("k", { type: "text_delta", text: "x" });
    s.setRequestId("k", "req-1");
    expect(useChatStore.getState().turns[0]).toMatchObject({
      requestId: "req-1",
      reply: "x",
    });
  });

  it("fail marks a streaming turn as error but keeps a finished one", () => {
    const s = useChatStore.getState();
    s.start("a", "1");
    s.start("b", "2");
    s.apply("b", { type: "done", stopReason: "end" });
    s.fail("a");
    s.fail("b");
    const [a, b] = useChatStore.getState().turns;
    expect(a.stopReason).toBe("error");
    expect(b.stopReason).toBe("end");
  });

  it("records why a turn was interrupted, even if the event beats the requestId", () => {
    const s = useChatStore.getState();
    s.start("a", "1");
    s.setRequestId("a", "req-a");
    s.markInterrupted("req-a", "hotkey");
    s.start("b", "2");
    s.markInterrupted("req-b", "wake_word"); // id not known yet
    expect(useChatStore.getState().pendingInterrupts).toEqual({
      "req-b": "wake_word",
    });
    s.setRequestId("b", "req-b");
    const [a, b] = useChatStore.getState().turns;
    expect([a.interruptedBy, b.interruptedBy]).toEqual(["hotkey", "wake_word"]);
    expect(useChatStore.getState().pendingInterrupts).toEqual({});
  });
});
