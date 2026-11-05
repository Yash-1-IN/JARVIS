import type { Snapshot } from "../types/generated/Snapshot";
import { PushToTalk } from "./PushToTalk";

export const MUTE_SHORTCUT_LABEL = "Ctrl+Shift+M";

interface Props {
  snapshot: Snapshot;
  onCancel: () => void;
  onToggleMute: () => void;
  onTalkStart: () => Promise<void>;
  onTalkStop: () => void;
}

export function Controls({
  snapshot,
  onCancel,
  onToggleMute,
  onTalkStart,
  onTalkStop,
}: Props) {
  const muted = snapshot.mic === "muted";
  return (
    <section aria-label="Controls" className="controls">
      <PushToTalk
        unavailable={snapshot.mic === "muted" || snapshot.mic === "unavailable"}
        busy={snapshot.activeRequestId !== null}
        listening={snapshot.phase === "listening"}
        micOpen={snapshot.mic === "capturing"}
        onStart={onTalkStart}
        onStop={onTalkStop}
      />
      <button
        type="button"
        aria-pressed={muted}
        aria-keyshortcuts="Control+Shift+M Meta+Shift+M"
        disabled={snapshot.mic === "unavailable"}
        onClick={onToggleMute}
      >
        {muted ? "Unmute" : "Mute"} <kbd>{MUTE_SHORTCUT_LABEL}</kbd>
      </button>
      <button
        type="button"
        aria-keyshortcuts="Escape"
        disabled={!snapshot.activeRequestId}
        onClick={onCancel}
      >
        Cancel <kbd>Esc</kbd>
      </button>
    </section>
  );
}
