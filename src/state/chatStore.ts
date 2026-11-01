import { create } from "zustand";
import type { StopReason } from "../types/generated/StopReason";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";

export interface ChatTurn {
  /** Client-side key; stream events can arrive before the backend's requestId. */
  key: string;
  requestId: string | null;
  user: string;
  reply: string;
  /** null while streaming. */
  stopReason: StopReason | null;
}

interface ChatStore {
  turns: ChatTurn[];
  start: (key: string, user: string) => void;
  setRequestId: (key: string, requestId: string) => void;
  /** Applies one stream event to the turn's transcript (display data only). */
  apply: (key: string, event: TurnStreamEvent) => void;
  fail: (key: string) => void;
}

const update = (turns: ChatTurn[], key: string, f: (t: ChatTurn) => ChatTurn) =>
  turns.map((t) => (t.key === key ? f(t) : t));

export const useChatStore = create<ChatStore>((set) => ({
  turns: [],
  start: (key, user) =>
    set((s) => ({
      turns: [
        ...s.turns,
        { key, requestId: null, user, reply: "", stopReason: null },
      ],
    })),
  setRequestId: (key, requestId) =>
    set((s) => ({ turns: update(s.turns, key, (t) => ({ ...t, requestId })) })),
  apply: (key, event) =>
    set((s) => ({
      turns: update(s.turns, key, (t) => {
        if (t.stopReason) return t; // nothing after `done`
        switch (event.type) {
          case "text_delta":
            return { ...t, reply: t.reply + event.text };
          case "done":
            return { ...t, stopReason: event.stopReason };
          default:
            // transcript / tool_proposed are rendered in M3 / M4.
            return t;
        }
      }),
    })),
  fail: (key) =>
    set((s) => ({
      turns: update(s.turns, key, (t) =>
        t.stopReason ? t : { ...t, stopReason: "error" },
      ),
    })),
}));
