import { useId, useState, type FormEvent } from "react";
import { codePoints } from "../hooks/useMemories";
import { MAX_MEMORY_CHARS } from "../state/memory";

interface Props {
  initial: string;
  saveLabel: string;
  busy: boolean;
  onSave: (text: string) => void;
  onCancel: () => void;
}

/** Edits a memory's text with the backend's limit (500 code points) shown live. */
export function MemoryEditor({
  initial,
  saveLabel,
  busy,
  onSave,
  onCancel,
}: Props) {
  const [text, setText] = useState(initial);
  const id = useId();
  const used = codePoints(text.trim());
  const tooLong = used > MAX_MEMORY_CHARS;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (used === 0 || tooLong) return;
    onSave(text.trim());
  };
  return (
    <form onSubmit={submit} className="memory-editor">
      <label htmlFor={id} className="visually-hidden">
        Memory text
      </label>
      <textarea
        id={id}
        value={text}
        rows={3}
        onChange={(e) => setText(e.target.value)}
        aria-describedby={`${id}-count`}
        aria-invalid={tooLong || undefined}
        autoFocus
      />
      <p
        id={`${id}-count`}
        className={tooLong ? "memory-count over" : "memory-count"}
      >
        {used} / {MAX_MEMORY_CHARS}
        {tooLong && " (too long)"}
      </p>
      <div className="memory-actions">
        <button type="submit" disabled={busy || used === 0 || tooLong}>
          {saveLabel}
        </button>
        <button type="button" disabled={busy} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}
