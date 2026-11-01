import { useDocumentHidden } from "../hooks/useDocumentHidden";
import type { AssistantPhase } from "../types/generated/AssistantPhase";
import type { MicState } from "../types/generated/MicState";

/**
 * Which animation each state gets. `none` means the orb is static, so an idle
 * window renders no animation frames (spec §13 M1).
 */
export type OrbMotion = "none" | "breathe" | "pulse" | "spin" | "wave";

export function orbMotion(phase: AssistantPhase, mic: MicState): OrbMotion {
  switch (phase) {
    case "listening":
      return "pulse";
    case "transcribing":
    case "thinking":
    case "acting":
      return "spin";
    case "speaking":
      return "wave";
    case "awaiting_approval":
      return "breathe";
    case "error":
    case "idle":
      // A live mic outside a turn is worth a slow cue; otherwise stay still.
      return mic === "capturing" ? "pulse" : "none";
  }
}

/**
 * The mandatory 2D orb (ADR-015): SVG + CSS only, no WebGL. Decorative; the
 * StatusPanel carries the text, so screen readers skip this. Colour is never
 * the only signal: each state also has its own shape and motion.
 * `prefers-reduced-motion` turns every animation off in CSS.
 */
export function Orb2D({
  phase,
  mic,
}: {
  phase: AssistantPhase;
  mic: MicState;
}) {
  const hidden = useDocumentHidden();
  const motion = orbMotion(phase, mic);
  return (
    <div
      className={`orb orb-${motion} orb-phase-${phase}${hidden ? " orb-paused" : ""}${
        mic === "muted" ? " orb-muted" : ""
      }`}
      data-testid="orb"
      data-motion={motion}
      aria-hidden="true"
    >
      <svg viewBox="0 0 100 100" width="120" height="120">
        <circle className="orb-core" cx="50" cy="50" r="28" />
        <circle className="orb-ring" cx="50" cy="50" r="40" />
        {phase === "error" && (
          <path className="orb-mark" d="M50 36 v18 M50 60 v4" />
        )}
        {mic === "muted" && <path className="orb-mark" d="M30 70 L70 30" />}
      </svg>
    </div>
  );
}
