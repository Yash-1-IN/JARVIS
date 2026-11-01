import { useCallback, useEffect, useRef, useState } from "react";
import { Chat } from "./components/Chat";
import { Controls } from "./components/Controls";
import { MockBadge } from "./components/MockBadge";
import { MockControls } from "./components/MockControls";
import { SettingsPanel } from "./components/SettingsPanel";
import { StatusPanel } from "./components/StatusPanel";
import { useShortcuts } from "./hooks/useShortcuts";
import { PROTOCOL_VERSION } from "./protocol";
import { useChatStore } from "./state/chatStore";
import { describeError } from "./state/errors";
import { useSnapshotStore } from "./state/snapshotStore";
import type { Transport } from "./transport";
import type { MockTransport } from "./transport/mock/MockTransport";

function isMock(t: Transport): t is MockTransport {
  return t.isMock;
}

export default function App({ transport }: { transport: Transport }) {
  const snapshot = useSnapshotStore((s) => s.snapshot);
  const receive = useSnapshotStore((s) => s.receive);
  const [error, setError] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  // Read the store at call time so the shortcut listener is bound once.
  const run = useCallback((p: Promise<void>) => {
    setCommandError(null);
    p.catch((e: unknown) => setCommandError(describeError(e)));
  }, []);
  const onCancel = useCallback(() => {
    const id = useSnapshotStore.getState().snapshot?.activeRequestId;
    if (id) run(transport.cancel(id));
  }, [transport, run]);
  const onToggleMute = useCallback(() => {
    const mic = useSnapshotStore.getState().snapshot?.mic;
    if (mic && mic !== "unavailable") run(transport.setMute(mic !== "muted"));
  }, [transport, run]);
  useShortcuts({ onCancel, onToggleMute });

  const turns = useChatStore((s) => s.turns);
  const nextKey = useRef(0);
  const onSubmitText = useCallback(
    (text: string) => {
      const chat = useChatStore.getState();
      const key = `turn-${++nextKey.current}`;
      chat.start(key, text);
      setCommandError(null);
      transport
        .submitText(text, (event) => useChatStore.getState().apply(key, event))
        .then((requestId) =>
          useChatStore.getState().setRequestId(key, requestId),
        )
        .catch((e: unknown) => {
          useChatStore.getState().fail(key);
          setCommandError(describeError(e));
        });
    },
    [transport],
  );

  // Retry resends the failed turn's text; offered only when that turn is known.
  const failedTurn = turns.find(
    (t) =>
      t.requestId !== null && t.requestId === snapshot?.lastError?.requestId,
  );
  const retryTurn = failedTurn
    ? () => onSubmitText(failedTurn.user)
    : undefined;

  useEffect(() => {
    let cancelled = false;
    // Subscribe before fetching so no update between the two is lost; the
    // store's seq check discards whichever arrives stale.
    const unsubscribe = transport.onSnapshot(receive);
    const offInterrupt = transport.onInterrupted((e) =>
      useChatStore.getState().markInterrupted(e.requestId, e.reason),
    );
    (async () => {
      try {
        const pong = await transport.ping();
        if (pong.protocolVersion !== PROTOCOL_VERSION) {
          throw new Error(
            `Protocol mismatch: frontend ${PROTOCOL_VERSION}, backend ${pong.protocolVersion}`,
          );
        }
        const initial = await transport.getSnapshot();
        if (!cancelled) receive(initial);
      } catch (e) {
        if (!cancelled) setError(describeError(e));
      }
    })();
    return () => {
      cancelled = true;
      unsubscribe();
      offInterrupt();
    };
  }, [transport, receive]);

  return (
    <main>
      {transport.isMock && <MockBadge />}
      <h1>JARVIS</h1>
      <button
        type="button"
        aria-expanded={showSettings}
        aria-controls="settings-panel"
        onClick={() => setShowSettings((v) => !v)}
      >
        Settings
      </button>
      {showSettings && (
        <div id="settings-panel">
          <SettingsPanel transport={transport} />
        </div>
      )}
      {error && <p role="alert">{error}</p>}
      {!error && snapshot && (
        <>
          <StatusPanel snapshot={snapshot} onRetry={retryTurn} />
          <Controls
            snapshot={snapshot}
            onCancel={onCancel}
            onToggleMute={onToggleMute}
          />
          <Chat
            turns={turns}
            busy={snapshot.activeRequestId !== null}
            onSubmit={onSubmitText}
          />
        </>
      )}
      {commandError && <p role="alert">{commandError}</p>}
      {isMock(transport) && <MockControls transport={transport} />}
    </main>
  );
}
