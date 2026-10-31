import { useEffect, useState } from "react";
import { PROTOCOL_VERSION } from "./protocol";
import { TauriTransport } from "./transport/tauri";
import type { Snapshot } from "./types/generated/Snapshot";

const transport = new TauriTransport();

export default function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const pong = await transport.ping();
        if (pong.protocolVersion !== PROTOCOL_VERSION) {
          setError(
            `Protocol mismatch: frontend ${PROTOCOL_VERSION}, backend ${pong.protocolVersion}`,
          );
          return;
        }
        setSnapshot(await transport.getSnapshot());
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  return (
    <main>
      <h1>JARVIS</h1>
      {error && <p role="alert">{error}</p>}
      {snapshot && (
        <p>
          Mic: {snapshot.mic} · Phase: {snapshot.phase}
        </p>
      )}
    </main>
  );
}
