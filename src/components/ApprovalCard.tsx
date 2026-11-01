import { useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { ApprovalRequest } from "../types/generated/ApprovalRequest";
import type { Decision } from "../types/generated/Decision";

interface Props {
  request: ApprovalRequest;
  /** Resolves when the backend accepted the decision; rejects otherwise. */
  onDecide: (decision: Decision) => Promise<void>;
}

function secondsLeft(expiresAt: number): number {
  return Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
}

/** Cards are keyed by approvalId, so `expiresAt` is fixed for a card's lifetime. */
function useSecondsLeft(expiresAt: number): number {
  const [secs, setSecs] = useState(() => secondsLeft(expiresAt));
  useEffect(() => {
    const id = setInterval(() => setSecs(secondsLeft(expiresAt)), 1000);
    return () => clearInterval(id);
  }, [expiresAt]);
  return secs;
}

/**
 * Approval card (spec §9). Security choices:
 * - The summary is the backend's sentence, rendered as plain text; the model never writes it.
 * - Targets are shown exactly, so the user sees what will be touched.
 * - Nothing is focused automatically, so a stray Enter or Space cannot approve.
 * - A decision is single-use: both buttons lock after the first click.
 * - After expiry nothing can be sent; the backend would reject it anyway.
 */
export function ApprovalCard({ request, onDecide }: Props) {
  const secs = useSecondsLeft(request.expiresAt);
  const [sent, setSent] = useState<Decision | null>(null);
  const [error, setError] = useState<string | null>(null);
  const expired = secs === 0;
  const locked = expired || sent !== null;
  const high = request.risk === "high";

  const decide = (d: Decision) => {
    if (locked) return;
    setSent(d);
    setError(null);
    onDecide(d).catch((e: unknown) => setError(describeError(e)));
  };

  return (
    <section
      className={`approval-card ${high ? "risk-high" : "risk-medium"}`}
      aria-labelledby={`appr-${request.approvalId}`}
      data-testid="approval-card"
    >
      <h2 id={`appr-${request.approvalId}`}>
        Approve this action?{" "}
        <span className="risk">{high ? "High risk" : "Medium risk"}</span>
      </h2>
      {/* Assertive: an action is waiting on the user. */}
      <p aria-live="assertive" className="approval-summary">
        {request.summary}
      </p>
      {request.targets.length > 0 && (
        <>
          <p className="approval-label">Affects:</p>
          <ul className="approval-targets">
            {request.targets.map((t) => (
              <li key={t}>
                <code>{t}</code>
              </li>
            ))}
          </ul>
        </>
      )}
      {request.tainted && (
        <p className="warning" data-testid="tainted-warning">
          This was suggested after JARVIS read outside content (a file or web
          page). Check that it's really what you want.
        </p>
      )}
      <p className="approval-voice">
        {request.allowVoice
          ? 'You can also say "yes" or "no".'
          : "On-screen approval only: voice can't approve this."}
      </p>
      <div className="approval-actions">
        <button type="button" disabled={locked} onClick={() => decide("deny")}>
          Deny
        </button>{" "}
        <button
          type="button"
          disabled={locked}
          onClick={() => decide("approve")}
        >
          Approve
        </button>{" "}
        <span role="timer" aria-live="off" data-testid="approval-timer">
          {expired ? "Expired" : `${secs}s left`}
        </span>
      </div>
      {sent && !error && (
        <p role="status">
          {sent === "approve"
            ? "Approved, running…"
            : "Denied. Nothing will run."}
        </p>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
