import { useState } from "react";
import { describeError } from "../state/errors";
import type { Memories } from "../hooks/useMemories";
import type { Memory } from "../types/generated/Memory";
import { MemoryEditor } from "./MemoryEditor";

function when(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

function MemoryItem({
  memory,
  actions,
}: {
  memory: Memory;
  actions: Pick<Memories, "accept" | "edit" | "remove">;
}) {
  const [mode, setMode] = useState<"view" | "edit" | "confirm-delete">("view");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<void>, after: typeof mode) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      setMode(after);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="memory-item" data-testid="memory-item">
      {mode === "edit" ? (
        <MemoryEditor
          initial={memory.content}
          saveLabel="Save"
          busy={busy}
          onCancel={() => setMode("view")}
          onSave={(text) =>
            void run(() => actions.edit(memory.id, text), "view")
          }
        />
      ) : (
        <>
          <p className="memory-text">{memory.content}</p>
          <p className="hint">
            {memory.status === "proposed" ? "Waiting for your answer · " : ""}
            Changed {when(memory.updatedAt)}
          </p>
          {mode === "confirm-delete" ? (
            <div className="memory-actions" role="group" aria-label="Confirm">
              <span>Delete this memory for good?</span>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void run(() => actions.remove(memory.id), "view")
                }
              >
                Delete
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => setMode("view")}
              >
                Keep it
              </button>
            </div>
          ) : (
            <div className="memory-actions">
              {memory.status === "proposed" && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(() => actions.accept(memory.id), "view")
                  }
                >
                  Keep
                </button>
              )}
              <button
                type="button"
                disabled={busy}
                onClick={() => setMode("edit")}
              >
                Edit
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => setMode("confirm-delete")}
              >
                Delete
              </button>
            </div>
          )}
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </li>
  );
}

/** Everything JARVIS remembers (spec §8.4): view, edit and delete for good. */
export function MemoryPanel({ data }: { data: Memories }) {
  const { memories, loadError } = data;
  if (loadError) return <p role="alert">{loadError}</p>;
  if (memories === null) return <p>Loading…</p>;
  const waiting = memories.filter((m) => m.status === "proposed");
  const kept = memories.filter((m) => m.status === "active");
  return (
    <div className="memory-panel">
      <p className="hint">
        Things you asked JARVIS to remember. Only kept memories are used, and
        deleting one removes it for good.
      </p>
      {waiting.length > 0 && (
        <>
          <h3 id="memory-waiting">Waiting for your answer</h3>
          <ul className="memory-list" aria-labelledby="memory-waiting">
            {waiting.map((m) => (
              <MemoryItem key={m.id} memory={m} actions={data} />
            ))}
          </ul>
        </>
      )}
      <h3 id="memory-kept">Kept</h3>
      {kept.length === 0 ? (
        <p data-testid="memory-empty">
          Nothing yet. Say or type &quot;remember that …&quot;.
        </p>
      ) : (
        <ul className="memory-list" aria-labelledby="memory-kept">
          {kept.map((m) => (
            <MemoryItem key={m.id} memory={m} actions={data} />
          ))}
        </ul>
      )}
    </div>
  );
}
