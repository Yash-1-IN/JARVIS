import { useCallback, useEffect, useRef, useState } from "react";
import { ActivityLog } from "./components/ActivityLog";
import { ApprovalCard } from "./components/ApprovalCard";
import { Chat } from "./components/Chat";
import { Controls } from "./components/Controls";
import { HistoryPanel } from "./components/HistoryPanel";
import { MemoryPanel } from "./components/MemoryPanel";
import { MemoryProposalCard } from "./components/MemoryProposalCard";
import { MockBadge } from "./components/MockBadge";
import { Hologram } from "./components/hologram/Hologram";
import { MockControls } from "./components/MockControls";
import { SettingsPanel } from "./components/SettingsPanel";
import { SidePanel } from "./components/SidePanel";
import { StatusPanel } from "./components/StatusPanel";
import { useMemories } from "./hooks/useMemories";
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

/** The panels that open beside the conversation; one at a time (issue #37). */
const PANELS = {
  memory: "Memory",
  history: "History",
  activity: "Activity",
  settings: "Settings",
} as const;
type PanelId = keyof typeof PANELS;

export default function App({ transport }: { transport: Transport }) {
  const snapshot = useSnapshotStore((s) => s.snapshot);
  const receive = useSnapshotStore((s) => s.receive);
  const [error, setError] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [panel, setPanel] = useState<PanelId | null>(null);
  const memories = useMemories(transport);
  const proposals = (memories.memories ?? []).filter(
    (m) => m.status === "proposed",
  );
  const navButtons = useRef<Partial<Record<PanelId, HTMLButtonElement | null>>>(
    {},
  );
  const togglePanel = (id: PanelId) =>
    setPanel((open) => (open === id ? null : id));
  // Closing returns focus to the button that opened the panel.
  const closePanel = () => {
    if (panel) navButtons.current[panel]?.focus();
    setPanel(null);
  };

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
    <div className={`app${panel ? " has-panel" : ""}`}>
      {transport.isMock && <MockBadge />}
      <header className="app-header">
        <h1>JARVIS</h1>
        <nav aria-label="Panels" className="app-nav">
          {(Object.keys(PANELS) as PanelId[]).map((id) => (
            <button
              key={id}
              type="button"
              ref={(el) => {
                navButtons.current[id] = el;
              }}
              aria-pressed={panel === id}
              aria-controls={panel === id ? `${id}-panel` : undefined}
              onClick={() => togglePanel(id)}
            >
              {PANELS[id]}
            </button>
          ))}
        </nav>
      </header>
      {/* Approval and memory cards sit above everything else, so an open panel never hides them. */}
      {((snapshot && snapshot.pendingApprovals.length > 0) ||
        proposals.length > 0) && (
        <div className="approvals">
          {snapshot?.pendingApprovals.map((r) => (
            <ApprovalCard
              key={r.approvalId}
              request={r}
              onDecide={(decision) =>
                transport.resolveApproval({
                  approvalId: r.approvalId,
                  argsDigest: r.argsDigest,
                  decision,
                })
              }
            />
          ))}
          {proposals.map((m) => (
            <MemoryProposalCard key={m.id} memory={m} actions={memories} />
          ))}
        </div>
      )}
      <div className="app-body">
        <main className="assistant">
          {error && <p role="alert">{error}</p>}
          {!error && snapshot && (
            <>
              <div className="assistant-status">
                <Hologram phase={snapshot.phase} mic={snapshot.mic} />
                <div>
                  <StatusPanel snapshot={snapshot} onRetry={retryTurn} />
                  <Controls
                    snapshot={snapshot}
                    onCancel={onCancel}
                    onToggleMute={onToggleMute}
                  />
                </div>
              </div>
              <Chat
                turns={turns}
                busy={snapshot.activeRequestId !== null}
                onSubmit={onSubmitText}
              />
            </>
          )}
          {commandError && <p role="alert">{commandError}</p>}
        </main>
        {panel && (
          <SidePanel
            id={`${panel}-panel`}
            title={PANELS[panel]}
            onClose={closePanel}
          >
            {panel === "settings" && <SettingsPanel transport={transport} />}
            {panel === "activity" && <ActivityLog transport={transport} />}
            {panel === "memory" && <MemoryPanel data={memories} />}
            {panel === "history" && <HistoryPanel transport={transport} />}
          </SidePanel>
        )}
      </div>
      {isMock(transport) && (
        <footer className="mock-footer">
          <MockControls transport={transport} />
        </footer>
      )}
    </div>
  );
}
