import { useState } from "react";
import { describeError } from "../state/errors";
import type { Memories } from "../hooks/useMemories";
import type { Memory } from "../types/generated/Memory";
import { MemoryEditor } from "./MemoryEditor";

interface Props {
  memory: Memory;
  actions: Pick<Memories, "accept" | "edit" | "remove">;
}

/**
 * "Remember this?" (spec §8.4). Nothing is kept until the user says so: Keep
 * accepts the exact text shown, Edit changes it first, Don't keep deletes it.
 * No button is focused automatically, so a stray Enter cannot keep anything.
 */
export function MemoryProposalCard({ memory, actions }: Props) {
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const titleId = `memory-card-${memory.id}`;

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(describeError(e));
      setBusy(false);
    }
  };

  return (
    <section
      className="memory-card"
      aria-labelledby={titleId}
      data-testid="memory-card"
    >
      <h2 id={titleId}>Remember this?</h2>
      {editing ? (
        <MemoryEditor
          initial={memory.content}
          saveLabel="Save and keep"
          busy={busy}
          onCancel={() => setEditing(false)}
          onSave={(text) =>
            void run(async () => {
              if (text !== memory.content) await actions.edit(memory.id, text);
              await actions.accept(memory.id);
            })
          }
        />
      ) : (
        <>
          {/* Exactly the text that will be kept, shown as text (never markdown). */}
          <blockquote className="memory-text">{memory.content}</blockquote>
          <div className="memory-actions">
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(() => actions.accept(memory.id))}
            >
              Keep
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => setEditing(true)}
            >
              Edit
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(() => actions.remove(memory.id))}
            >
              Don&apos;t keep
            </button>
          </div>
          <p className="hint">
            JARVIS uses kept memories to answer you. You can change or delete
            them any time under Memory.
          </p>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
