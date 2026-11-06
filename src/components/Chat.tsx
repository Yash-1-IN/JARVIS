import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
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
  /** Clears the conversation on the screen. Omitted: no Clear button. */
  onClear?: () => void;
}

/** Within this many pixels of the bottom counts as "at the bottom". */
const BOTTOM_SLACK = 48;

export function Chat({ turns, busy, onSubmit, onClear }: Props) {
  const [draft, setDraft] = useState("");
  const scroller = useRef<HTMLDivElement>(null);
  // Follows the newest text while the person is at the bottom; if they scroll up to read, it stays
  // where they are and offers a way back down instead of pulling the page away.
  const atBottom = useRef(true);
  const [behind, setBehind] = useState(false);

  const toBottom = useCallback(() => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    atBottom.current = true;
    setBehind(false);
  }, []);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const near =
      el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK;
    atBottom.current = near;
    if (near) setBehind(false);
  };

  // Runs after every change to the conversation (a new turn, a streamed word).
  useLayoutEffect(() => {
    if (atBottom.current) toBottom();
    else setBehind(true);
  }, [turns, toBottom]);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    onSubmit(text);
    setDraft("");
  };

  return (
    <section aria-label="Conversation" className="chat">
      <div className="chat-head">
        <span className="chat-title">Conversation</span>
        {behind && (
          <button type="button" onClick={toBottom}>
            Jump to latest
          </button>
        )}
        {onClear && (
          <button
            type="button"
            className="chat-clear"
            disabled={turns.length === 0}
            title="Clears the screen only. Past conversations stay in History."
            onClick={() => {
              onClear();
              atBottom.current = true;
              setBehind(false);
            }}
          >
            Clear chat
          </button>
        )}
      </div>
      <div className="chat-scroll" ref={scroller} onScroll={onScroll}>
        <ol className="chat-log">
          {turns.map((t) => (
            <li key={t.key}>
              <p className="chat-user">
                <span className="visually-hidden">You: </span>
                {t.user}
              </p>
              {/* Polite live region: replies are read once they settle, not per token. */}
              <div
                className={`chat-reply${t.stopReason === null ? " chat-reply-streaming" : ""}`}
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
      </div>
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
