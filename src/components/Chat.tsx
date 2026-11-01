import { useState, type FormEvent } from "react";
import type { ChatTurn } from "../state/chatStore";
import { Markdown } from "./Markdown";

const STOP_NOTES: Partial<Record<NonNullable<ChatTurn["stopReason"]>, string>> =
  {
    cancelled: "Cancelled",
    error: "Stopped by an error",
    spend_cap: "Stopped: spend cap reached",
  };

const INTERRUPT_NOTES: Record<
  NonNullable<ChatTurn["interruptedBy"]>,
  string
> = {
  user_cancel: "Cancelled",
  hotkey: "Stopped by the hotkey",
  wake_word: "Interrupted: listening again",
  error: "Stopped by an error",
};

/** The interrupt reason, when known, says more than the bare stop reason. */
function stopNote(t: ChatTurn): string | undefined {
  if (t.interruptedBy) return INTERRUPT_NOTES[t.interruptedBy];
  return t.stopReason ? STOP_NOTES[t.stopReason] : undefined;
}

interface Props {
  turns: ChatTurn[];
  busy: boolean;
  onSubmit: (text: string) => void;
}

export function Chat({ turns, busy, onSubmit }: Props) {
  const [draft, setDraft] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    onSubmit(text);
    setDraft("");
  };

  return (
    <section aria-label="Conversation" className="chat">
      <ol className="chat-log">
        {turns.map((t) => (
          <li key={t.key}>
            <p className="chat-user">
              <span className="visually-hidden">You: </span>
              {t.user}
            </p>
            {/* Polite live region: replies are read once they settle, not per token. */}
            <div
              className="chat-reply"
              aria-live="polite"
              aria-busy={t.stopReason === null}
            >
              <span className="visually-hidden">JARVIS: </span>
              <Markdown text={t.reply} />
              {stopNote(t) && (
                <p>
                  <em className="chat-stop">[{stopNote(t)}]</em>
                </p>
              )}
            </div>
          </li>
        ))}
      </ol>
      <form onSubmit={submit} className="chat-form">
        <label htmlFor="chat-input" className="visually-hidden">
          Message JARVIS
        </label>
        <input
          id="chat-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Type a message"
          autoComplete="off"
        />
        <button type="submit" disabled={busy || !draft.trim()}>
          Send
        </button>
      </form>
    </section>
  );
}
