import { useCallback, useEffect, useState } from "react";
import { Controls } from "./components/Controls";
import { MockBadge } from "./components/MockBadge";
import { MockControls } from "./components/MockControls";
import { StatusPanel } from "./components/StatusPanel";
import { useShortcuts } from "./hooks/useShortcuts";
import { PROTOCOL_VERSION } from "./protocol";
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

  // Read the store at call time so the shortcut listener is bound once.
  const run = useCallback((p: Promise<void>) => {
    setCommandError(null);
    p.catch((e: unknown) => setCommandError(String(e)));
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

  useEffect(() => {
    let cancelled = false;
    // Subscribe before fetching so no update between the two is lost; the
    // store's seq check discards whichever arrives stale.
    const unsubscribe = transport.onSnapshot(receive);
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
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [transport, receive]);

  return (
    <main>
      {transport.isMock && <MockBadge />}
      <h1>JARVIS</h1>
      {error && <p role="alert">{error}</p>}
      {!error && snapshot && (
        <>
          <StatusPanel snapshot={snapshot} />
          <Controls
            snapshot={snapshot}
            onCancel={onCancel}
            onToggleMute={onToggleMute}
          />
        </>
      )}
      {commandError && <p role="alert">{commandError}</p>}
      {isMock(transport) && <MockControls transport={transport} />}
    </main>
  );
}
