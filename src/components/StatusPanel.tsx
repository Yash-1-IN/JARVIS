import { MIC_LABELS, NETWORK_LABELS, PHASE_LABELS } from "../state/labels";
import type { Snapshot } from "../types/generated/Snapshot";

export function StatusPanel({ snapshot }: { snapshot: Snapshot }) {
  const approval = snapshot.pendingApprovals[0];
  return (
    <section aria-label="Assistant status" className="status-panel">
      {/* Polite so state changes are announced without interrupting speech. */}
      <p aria-live="polite" aria-atomic="true">
        <span data-testid="phase" data-phase={snapshot.phase}>
          {PHASE_LABELS[snapshot.phase]}
        </span>
        {" · "}
        <span data-testid="mic" data-mic={snapshot.mic}>
          {MIC_LABELS[snapshot.mic]}
        </span>
      </p>
      <p>
        <span data-testid="network">{NETWORK_LABELS[snapshot.network]}</span>
        {snapshot.cloudInUse && (
          <>
            {" "}
            · <strong>Sending data to the cloud</strong>
          </>
        )}
      </p>
      {snapshot.lastError && <p role="alert">{snapshot.lastError.message}</p>}
      {approval && (
        // Approval card UI (buttons, expiry countdown) lands in a later M1 slice.
        <p data-testid="approval">
          Approval needed ({approval.risk} risk): {approval.summary}
        </p>
      )}
    </section>
  );
}
