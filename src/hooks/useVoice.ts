import { useEffect } from "react";
import { describeError } from "../state/errors";
import { useChatStore } from "../state/chatStore";
import type { Transport } from "../transport";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";

/** How long events that arrive before their transcript wait for it, in milliseconds. */
export const ORPHAN_MS = 2000;

/**
 * Connects voice-started turns to the conversation (contract `voice_attach`).
 *
 * The window attaches one channel at start-up. `transcript.final` (a broadcast event) opens the
 * turn with the heard words, and the events that then arrive on the attached channel belong to
 * it until its `done`. The two travel separately and may arrive in either order, so channel events
 * that come first wait briefly for their transcript. A capture that ends without hearing anything
 * (no model, no speech, a lost microphone) has no transcript and adds nothing to the conversation;
 * the error is shown from the snapshot like any other.
 */
export function useVoice(
  transport: Transport,
  onError: (message: string) => void,
) {
  useEffect(() => {
    let openKey: string | null = null;
    let waiting: TurnStreamEvent[] = [];
    let dropTimer: ReturnType<typeof setTimeout> | undefined;
    let count = 0;

    const deliver = (event: TurnStreamEvent) => {
      if (openKey === null) return;
      useChatStore.getState().apply(openKey, event);
      if (event.type === "done") openKey = null;
    };

    const off = transport.onTranscriptFinal((t) => {
      clearTimeout(dropTimer);
      const key = `voice-${++count}`;
      const chat = useChatStore.getState();
      chat.start(key, t.text);
      chat.setRequestId(key, t.requestId);
      openKey = key;
      const early = waiting;
      waiting = [];
      early.forEach(deliver);
    });

    transport
      .voiceAttach((event) => {
        if (openKey !== null) {
          deliver(event);
          return;
        }
        waiting.push(event);
        if (event.type === "done") {
          // No transcript came with this turn: forget it, so it cannot leak into the next one.
          clearTimeout(dropTimer);
          dropTimer = setTimeout(() => {
            waiting = [];
          }, ORPHAN_MS);
        }
      })
      .catch((e: unknown) => onError(describeError(e)));

    return () => {
      off();
      clearTimeout(dropTimer);
    };
  }, [transport, onError]);
}
