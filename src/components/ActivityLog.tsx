import { useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { ActivityEntry } from "../types/generated/ActivityEntry";
import type { ToolDecision } from "../types/generated/ToolDecision";
import type { ToolOutcome } from "../types/generated/ToolOutcome";

const PAGE = 50;

// Words, not colours, carry the meaning (accessibility, spec §13 M1).
const DECISION: Record<ToolDecision, string> = {
  auto_allowed: "Ran without asking",
  approved: "You approved",
  denied: "You denied",
  expired: "Approval expired",
  prohibited: "Refused",
};

const OUTCOME: Record<ToolOutcome, string> = {
  ok: "done",
  failed: "failed",
  timed_out: "timed out",
  cancelled: "cancelled",
  not_run: "not run",
};

function when(ts: number): string {
  return new Date(ts).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/** Newest first; a new entry from `activity.appended` goes on top (deduped by id). */
function merge(top: ActivityEntry[], rest: ActivityEntry[]): ActivityEntry[] {
  const seen = new Set<string>();
  return [...top, ...rest].filter((e) => !seen.has(e.id) && seen.add(e.id));
}

/**
 * The activity log (spec §9.3): every tool call, including refused, denied and expired
 * ones. Entries never contain arguments or file contents; everything is plain text.
 */
export function ActivityLog({ transport }: { transport: Transport }) {
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let live = true;
    const off = transport.onActivity(
      (e) => live && setEntries((cur) => merge([e], cur ?? [])),
    );
    transport
      .listActivity(PAGE)
      .then((page) => {
        if (!live) return;
        setEntries((cur) => merge(cur ?? [], page));
        setMore(page.length === PAGE);
      })
      .catch((e: unknown) => live && setError(describeError(e)));
    return () => {
      live = false;
      off();
    };
  }, [transport]);

  const older = async () => {
    const last = entries?.[entries.length - 1];
    if (!last) return;
    setLoading(true);
    try {
      const page = await transport.listActivity(PAGE, last.id);
      setEntries((cur) => merge(cur ?? [], page));
      setMore(page.length === PAGE);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <section aria-label="Activity" className="activity">
      <h2>Activity</h2>
      <p className="hint">
        Everything JARVIS did or tried to do on your computer.
      </p>
      {error && <p role="alert">{error}</p>}
      {entries === null && !error && <p>Loading…</p>}
      {entries?.length === 0 && (
        <p data-testid="activity-empty">Nothing yet.</p>
      )}
      {entries && entries.length > 0 && (
        <ol className="activity-list" aria-label="Activity entries">
          {entries.map((e) => (
            <li
              key={e.id}
              data-testid="activity-entry"
              data-decision={e.decision}
            >
              <span className="activity-when">{when(e.ts)}</span>{" "}
              <strong>{DECISION[e.decision]}</strong>
              {e.decision !== "prohibited" && e.outcome !== "not_run" && (
                <>, {OUTCOME[e.outcome]}</>
              )}
              {" · "}
              <span className="activity-risk">{e.risk} risk</span>
              <br />
              <span>{e.summary}</span>{" "}
              <code className="activity-tool">{e.tool}</code>
              {e.targets.length > 0 && (
                <ul className="activity-targets">
                  {e.targets.map((t) => (
                    <li key={t}>
                      <code>{t}</code>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ol>
      )}
      {more && (
        <button type="button" disabled={loading} onClick={() => void older()}>
          Show older
        </button>
      )}
    </section>
  );
}
