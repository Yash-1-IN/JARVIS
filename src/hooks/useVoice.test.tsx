import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useChatStore } from "../state/chatStore";
import type { Transport, TurnListener, TranscriptListener } from "../transport";
import { ORPHAN_MS, useVoice } from "./useVoice";

function fakeTransport() {
  let stream: TurnListener = () => {};
  let transcript: TranscriptListener = () => {};
  const t = {
    voiceAttach: vi.fn(async (l: TurnListener) => {
      stream = l;
    }),
    onTranscriptFinal: vi.fn((l: TranscriptListener) => {
      transcript = l;
      return () => {};
    }),
  } as unknown as Transport;
  return {
    t,
    stream: (e: Parameters<TurnListener>[0]) => stream(e),
    heard: (requestId: string, text: string) => transcript({ requestId, text }),
  };
}

function Host({ t }: { t: Transport }) {
  useVoice(t, () => {});
  return null;
}

describe("useVoice", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });
  afterEach(() => vi.useRealTimers());

  it("opens a turn with the heard words and streams the reply into it", async () => {
    const f = fakeTransport();
    render(<Host t={f.t} />);
    await act(async () => {});
    act(() => {
      f.heard("r1", "what time is it");
      f.stream({ type: "text_delta", text: "Noon." });
      f.stream({ type: "done", stopReason: "end" });
    });
    const [turn] = useChatStore.getState().turns;
    expect(turn).toMatchObject({
      user: "what time is it",
      requestId: "r1",
      reply: "Noon.",
      stopReason: "end",
    });
  });

  it("keeps channel events that arrive before their transcript", async () => {
    const f = fakeTransport();
    render(<Host t={f.t} />);
    await act(async () => {});
    act(() => {
      f.stream({ type: "text_delta", text: "Noon." });
      f.stream({ type: "done", stopReason: "end" });
      f.heard("r1", "what time is it");
    });
    expect(useChatStore.getState().turns[0]).toMatchObject({
      reply: "Noon.",
      stopReason: "end",
    });
  });

  it("does not let a turn that heard nothing leak into the next one", async () => {
    const f = fakeTransport();
    render(<Host t={f.t} />);
    await act(async () => {});
    act(() => {
      f.stream({ type: "text_delta", text: "stale" });
      f.stream({ type: "done", stopReason: "error" });
      vi.advanceTimersByTime(ORPHAN_MS + 1);
      f.heard("r2", "next");
      f.stream({ type: "text_delta", text: "fresh" });
      f.stream({ type: "done", stopReason: "end" });
    });
    const turns = useChatStore.getState().turns;
    expect(turns).toHaveLength(1);
    expect(turns[0].reply).toBe("fresh");
  });

  it("starts a second turn after the first is done", async () => {
    const f = fakeTransport();
    render(<Host t={f.t} />);
    await act(async () => {});
    act(() => {
      f.heard("r1", "one");
      f.stream({ type: "done", stopReason: "end" });
      f.heard("r2", "two");
      f.stream({ type: "text_delta", text: "b" });
      f.stream({ type: "done", stopReason: "end" });
    });
    const turns = useChatStore.getState().turns;
    expect(turns.map((t) => [t.user, t.reply])).toEqual([
      ["one", ""],
      ["two", "b"],
    ]);
  });
});
