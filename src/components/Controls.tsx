import type { Snapshot } from "../types/generated/Snapshot";

export const MUTE_SHORTCUT_LABEL = "Ctrl+Shift+M";

interface Props {
  snapshot: Snapshot;
  onCancel: () => void;
  onToggleMute: () => void;
}

export function Controls({ snapshot, onCancel, onToggleMute }: Props) {
  const muted = snapshot.mic === "muted";
  return (
    <section aria-label="Controls" className="controls">
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
