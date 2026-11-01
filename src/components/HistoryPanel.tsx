import { useCallback, useEffect, useState } from "react";
import { saveTextFile } from "../state/download";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { ConversationSummary } from "../types/generated/ConversationSummary";
import type { HistoryMessage } from "../types/generated/HistoryMessage";
import type { HistoryRetention } from "../types/generated/HistoryRetention";
import type { HistoryRole } from "../types/generated/HistoryRole";

const PAGE = 30;

const RETENTION: Record<HistoryRetention, string> = {
  off: "Off (keep nothing)",
  days7: "7 days",
  days30: "30 days",
  days365: "1 year",
  forever: "Forever",
};
/** Shorter first; a move left in this list deletes history at once. */
const RETENTION_ORDER: HistoryRetention[] = [
  "off",
  "days7",
  "days30",
  "days365",
  "forever",
];

const ROLE: Record<HistoryRole, string> = {
  user: "You",
  assistant: "JARVIS",
  tool: "Action",
};

function when(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

type Confirm =
  { kind: "retention"; to: HistoryRetention } | { kind: "delete-all" } | null;

/** Conversation history (spec §8.2, §8.4): read, keep for how long, delete, export. */
export function HistoryPanel({ transport }: { transport: Transport }) {
  const [list, setList] = useState<ConversationSummary[] | null>(null);
  const [more, setMore] = useState(false);
  const [retention, setRetention] = useState<HistoryRetention | null>(null);
  const [open, setOpen] = useState<{
    summary: ConversationSummary;
    messages: HistoryMessage[];
  } | null>(null);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    const [page, settings] = await Promise.all([
      transport.listConversations(PAGE),
      transport.getHistorySettings(),
    ]);
    setList(page);
    setMore(page.length === PAGE);
    setRetention(settings.retention);
  }, [transport]);

  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() =>
        Promise.all([
          transport.listConversations(PAGE),
          transport.getHistorySettings(),
        ]),
      )
      .then(([page, settings]) => {
        if (!live) return;
        setList(page);
        setMore(page.length === PAGE);
        setRetention(settings.retention);
      })
      .catch((e: unknown) => live && setError(describeError(e)));
    return () => {
      live = false;
    };
  }, [transport]);

  const run = async (action: () => Promise<void>, done?: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (done) setNotice(done);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const older = () =>
    run(async () => {
      const last = list?.[list.length - 1];
      if (!last) return;
      const page = await transport.listConversations(PAGE, last.id);
      setList((l) => [...(l ?? []), ...page]);
      setMore(page.length === PAGE);
    });

  const show = (c: ConversationSummary) =>
    run(async () => {
      setOpen({ summary: c, messages: await transport.getConversation(c.id) });
    });

  const chooseRetention = (to: HistoryRetention) => {
    if (!retention || to === retention) return;
    const shorter =
      RETENTION_ORDER.indexOf(to) < RETENTION_ORDER.indexOf(retention);
    if (shorter) setConfirm({ kind: "retention", to });
    else void saveRetention(to);
  };

  const saveRetention = (to: HistoryRetention) =>
    run(async () => {
      setConfirm(null);
      const s = await transport.setHistorySettings({ retention: to });
      setRetention(s.retention);
      await reload();
    }, "History setting saved.");

  const deleteAll = () =>
    run(async () => {
      setConfirm(null);
      await transport.deleteAllHistory();
      setOpen(null);
      await reload();
    }, "All conversation history deleted.");

  const exportAll = () =>
    run(async () => {
      const json = await transport.exportData();
      saveTextFile(`jarvis-export-${today()}.json`, json, "application/json");
    }, "Export saved to your downloads.");

  if (open) {
    return (
      <div className="history">
        <button type="button" onClick={() => setOpen(null)}>
          ← All conversations
        </button>
        <h3>{open.summary.title ?? "Untitled conversation"}</h3>
        <p className="hint">{when(open.summary.startedAt)}</p>
        {/* Stored text is shown as plain text, never markdown or HTML. */}
        <ol className="history-messages" aria-label="Messages">
          {open.messages.map((m) => (
            <li key={m.id} data-role={m.role}>
              <span className="history-role">{ROLE[m.role]}</span>{" "}
              <span className="history-when">{when(m.ts)}</span>
              <p className="history-text">{m.content}</p>
            </li>
          ))}
        </ol>
        {error && <p role="alert">{error}</p>}
      </div>
    );
  }

  return (
    <div className="history">
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}

      <h3>Keep history for</h3>
      {retention && (
        <>
          <label htmlFor="history-retention" className="visually-hidden">
            Keep history for
          </label>
          <select
            id="history-retention"
            value={retention}
            disabled={busy}
            onChange={(e) =>
              chooseRetention(e.target.value as HistoryRetention)
            }
          >
            {RETENTION_ORDER.map((r) => (
              <option key={r} value={r}>
                {RETENTION[r]}
              </option>
            ))}
          </select>
        </>
      )}
      {confirm?.kind === "retention" && (
        <div className="confirm" role="group" aria-label="Confirm">
          <p>
            {confirm.to === "off"
              ? "Turning history off deletes every saved conversation now."
              : `Conversations older than ${RETENTION[confirm.to]} are deleted now.`}
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={() => void saveRetention(confirm.to)}
          >
            Change and delete
          </button>{" "}
          <button type="button" onClick={() => setConfirm(null)}>
            Cancel
          </button>
        </div>
      )}
      <p className="hint">
        History stays on this computer. Memories are separate and are kept until
        you delete them.
      </p>

      <h3>Conversations</h3>
      {list === null && !error && <p>Loading…</p>}
      {list?.length === 0 && <p data-testid="history-empty">Nothing saved.</p>}
      {list && list.length > 0 && (
        <ul className="history-list" aria-label="Conversations">
          {list.map((c) => (
            <li key={c.id}>
              <button
                type="button"
                className="history-open"
                disabled={busy}
                onClick={() => void show(c)}
              >
                <span className="history-title">
                  {c.title ?? "Untitled conversation"}
                </span>
                <span className="hint">
                  {when(c.startedAt)} · {c.messageCount}{" "}
                  {c.messageCount === 1 ? "message" : "messages"}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {more && (
        <button type="button" disabled={busy} onClick={() => void older()}>
          Show older
        </button>
      )}

      <h3>Your data</h3>
      <div className="memory-actions">
        <button type="button" disabled={busy} onClick={() => void exportAll()}>
          Export memories and history
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => setConfirm({ kind: "delete-all" })}
        >
          Delete all history
        </button>
      </div>
      {confirm?.kind === "delete-all" && (
        <div className="confirm" role="group" aria-label="Confirm">
          <p>
            Delete every saved conversation for good? Memories stay. A reply in
            progress is stopped.
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={() => void deleteAll()}
          >
            Delete all
          </button>{" "}
          <button type="button" onClick={() => setConfirm(null)}>
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}
