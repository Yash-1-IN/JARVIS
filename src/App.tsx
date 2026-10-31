import { useEffect, useState } from "react";
import { MockBadge } from "./components/MockBadge";
import { MockControls } from "./components/MockControls";
import { StatusPanel } from "./components/StatusPanel";
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
      {!error && snapshot && <StatusPanel snapshot={snapshot} />}
      {isMock(transport) && <MockControls transport={transport} />}
    </main>
  );
}
