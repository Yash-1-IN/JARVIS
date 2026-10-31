import { useEffect } from "react";

interface Handlers {
  onCancel: () => void;
  onToggleMute: () => void;
}

/** Esc cancels; Ctrl/Cmd+Shift+M toggles mute. */
export function useShortcuts({ onCancel, onToggleMute }: Handlers) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat) return;
      if (e.key === "Escape") {
        onCancel();
      } else if (
        (e.ctrlKey || e.metaKey) &&
        e.shiftKey &&
        e.key.toLowerCase() === "m"
      ) {
        e.preventDefault();
        onToggleMute();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, onToggleMute]);
}
