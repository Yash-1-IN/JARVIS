import { useCallback, useEffect, useRef, useState } from "react";

interface Props {
  /** Nothing can be heard: the microphone is muted or unavailable. */
  unavailable: boolean;
  /** Another request is running (and it is not this capture). */
  busy: boolean;
  /** The microphone has opened and is delivering sound (`mic` is `capturing`): speak now. */
  micOpen: boolean;
  /** Key down. A rejection puts the button back; the caller shows the error. */
  onStart: () => Promise<void>;
  /** Key up, or anything that ends the press. Safe to call when nothing is capturing. */
  onStop: () => void;
}

/**
 * Hold to talk. The press ends on release, on a cancelled pointer, when focus leaves the button and
 * when the window is hidden, so a lost key-up cannot leave the microphone open (contract: the
 * backend also stops at 15 seconds). Space and Enter work while the button has focus.
 */
export function PushToTalk({
  unavailable,
  busy,
  micOpen,
  onStart,
  onStop,
}: Props) {
  const [holding, setHolding] = useState(false);
  const held = useRef(false);

  const stop = useCallback(() => {
    if (!held.current) return;
    held.current = false;
    setHolding(false);
    onStop();
  }, [onStop]);

  const start = () => {
    if (held.current) return;
    held.current = true;
    setHolding(true);
    onStart().catch(() => {
      held.current = false;
      setHolding(false);
    });
  };

  useEffect(() => {
    const onHide = () => {
      if (document.hidden) stop();
    };
    document.addEventListener("visibilitychange", onHide);
    return () => document.removeEventListener("visibilitychange", onHide);
  }, [stop]);

  // Muting (button, hotkey) while the key is down ends the capture; tell the backend too.
  useEffect(() => {
    if (unavailable) stop();
  }, [unavailable, stop]);

  const disabled = unavailable || (busy && !holding);
  return (
    <button
      type="button"
      className="push-to-talk"
      aria-pressed={holding}
      aria-keyshortcuts="Space"
      disabled={disabled}
      onPointerDown={(e) => {
        if (e.button > 0) return;
        e.currentTarget.setPointerCapture?.(e.pointerId);
        start();
      }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onBlur={stop}
      onKeyDown={(e) => {
        if (e.repeat || (e.key !== " " && e.key !== "Enter")) return;
        e.preventDefault();
        start();
      }}
      onKeyUp={(e) => {
        if (e.key === " " || e.key === "Enter") stop();
      }}
    >
      {holding
        ? micOpen
          ? "Speak now… release to send"
          : "Starting microphone…"
        : "Hold to talk"}
    </button>
  );
}
