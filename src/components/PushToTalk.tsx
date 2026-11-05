import { useCallback, useEffect, useRef, useState } from "react";

/** A press shorter than this is a tap: the capture keeps running after the release. */
export const TAP_MS = 300;
/** A tap-started capture that the backend never ended (it always does, within 20 s) is let go. */
const TAP_GIVE_UP_MS = 25_000;

interface Props {
  /** Nothing can be heard: the microphone is muted or unavailable. */
  unavailable: boolean;
  /** Another request is running (and it is not this capture). */
  busy: boolean;
  /** The assistant is in the `listening` phase (a capture is running). */
  listening: boolean;
  /** The microphone has opened and is delivering sound (`mic` is `capturing`): speak now. */
  micOpen: boolean;
  /** Key down. A rejection puts the button back; the caller shows the error. */
  onStart: () => Promise<void>;
  /** Send what was said so far, or end the capture. Safe to call when nothing is capturing. */
  onStop: () => void;
}

type Mode = "idle" | "held" | "tap";

/**
 * Talk to JARVIS two ways. **Hold**: the capture runs while the button is down and the release
 * sends what was said. **Tap** (down and up within about 300 ms): the capture keeps running and the
 * backend ends it by itself when you stop talking (a quiet gap, the 15 s cap, or nothing heard);
 * pressing again sends what was said so far. Esc and mute end either at once, and so does hiding
 * the window; focus leaving the button ends a hold only (a tap lets you click elsewhere). Space and
 * Enter work while the button has focus. A forgotten tap cannot leave the microphone open: the
 * backend bounds every capture (contract: "Push to talk and a lost key-up").
 */
export function PushToTalk({
  unavailable,
  busy,
  listening,
  micOpen,
  onStart,
  onStop,
}: Props) {
  const [mode, setModeState] = useState<Mode>("idle");
  const modeRef = useRef<Mode>("idle");
  const pressedAt = useRef(0);
  const sawListening = useRef(false);
  const setMode = (m: Mode) => {
    modeRef.current = m;
    setModeState(m);
  };

  /** Ends whatever is running and tells the backend. */
  const end = useCallback(() => {
    if (modeRef.current === "idle") return;
    modeRef.current = "idle";
    setModeState("idle");
    onStop();
  }, [onStop]);

  const down = () => {
    if (modeRef.current === "tap") {
      end(); // a second press sends what was said so far
      return;
    }
    if (modeRef.current === "held") return;
    pressedAt.current = Date.now();
    sawListening.current = false;
    setMode("held");
    onStart().catch(() => setMode("idle"));
  };

  const up = () => {
    if (modeRef.current !== "held") return;
    if (Date.now() - pressedAt.current < TAP_MS) {
      setMode("tap"); // the capture carries on by itself
    } else {
      end();
    }
  };

  useEffect(() => {
    const onHide = () => {
      if (document.hidden) end();
    };
    document.addEventListener("visibilitychange", onHide);
    return () => document.removeEventListener("visibilitychange", onHide);
  }, [end]);

  // Muting (button, hotkey) ends the capture; tell the backend too.
  useEffect(() => {
    if (unavailable) end();
  }, [unavailable, end]);

  // A tap-started capture ends on its own (end of speech, Esc, an error): let the button go back.
  useEffect(() => {
    if (listening) sawListening.current = true;
    else if (mode === "tap" && sawListening.current) setMode("idle");
  }, [listening, mode]);
  useEffect(() => {
    if (mode !== "tap") return;
    const t = setTimeout(() => setMode("idle"), TAP_GIVE_UP_MS);
    return () => clearTimeout(t);
  }, [mode]);

  const active = mode !== "idle";
  const disabled = unavailable || (busy && !active);
  return (
    <button
      type="button"
      className="push-to-talk"
      aria-pressed={active}
      aria-keyshortcuts="Space"
      disabled={disabled}
      onPointerDown={(e) => {
        if (e.button > 0) return;
        e.currentTarget.setPointerCapture?.(e.pointerId);
        down();
      }}
      onPointerUp={up}
      onPointerCancel={() => end()}
      onBlur={() => {
        if (modeRef.current === "held") end();
      }}
      onKeyDown={(e) => {
        if (e.repeat || (e.key !== " " && e.key !== "Enter")) return;
        e.preventDefault();
        down();
      }}
      onKeyUp={(e) => {
        if (e.key === " " || e.key === "Enter") up();
      }}
    >
      {mode === "idle"
        ? "Hold or tap to talk"
        : !micOpen
          ? "Starting microphone…"
          : mode === "tap"
            ? "Speak now… press to send"
            : "Speak now… release to send"}
    </button>
  );
}
