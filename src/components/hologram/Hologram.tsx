import { Component, lazy, Suspense, useState, type ReactNode } from "react";
import { useDarkScheme } from "../../hooks/useDarkScheme";
import { useDocumentHidden } from "../../hooks/useDocumentHidden";
import { useReducedMotion } from "../../hooks/useReducedMotion";
import type { AssistantPhase } from "../../types/generated/AssistantPhase";
import type { MicState } from "../../types/generated/MicState";
import { Orb2D, orbMotion } from "../Orb2D";
import {
  chooseRenderer,
  frameloopFor,
  hasWebGL,
  isLinux,
  useHologramPref,
} from "./renderer";

// Loaded only when 3D is chosen, so the 2D path never downloads Three.js.
const Hologram3D = lazy(() => import("./Hologram3D"));

/** Any failure inside the 3D tree (no context, shader error) shows the 2D orb. */
class Fallback extends Component<
  { fallback: ReactNode; onError: () => void; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch() {
    this.props.onError();
  }
  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

// Checked once: creating a WebGL context is not free.
let webglCache: boolean | null = null;
function webglAvailable(): boolean {
  webglCache ??= hasWebGL();
  return webglCache;
}

/**
 * The assistant's hologram (spec §13 M1, ADR-015): 3D where it is safe, the
 * mandatory 2D orb otherwise. Both take the same props and are decorative;
 * the status text carries the meaning.
 */
export function Hologram({
  phase,
  mic,
}: {
  phase: AssistantPhase;
  mic: MicState;
}) {
  const pref = useHologramPref((s) => s.pref);
  const hidden = useDocumentHidden();
  const reduced = useReducedMotion();
  const dark = useDarkScheme();
  // Set when the 3D view failed this session; 2D from then on.
  const [broken, setBroken] = useState(false);
  const renderer = broken
    ? "2d"
    : chooseRenderer(pref, { webgl: webglAvailable(), linux: isLinux() });

  const flat = <Orb2D phase={phase} mic={mic} />;
  if (renderer === "2d") return flat;
  const motion = orbMotion(phase, mic);
  return (
    <Fallback fallback={flat} onError={() => setBroken(true)}>
      <Suspense fallback={flat}>
        <Hologram3D
          phase={phase}
          mic={mic}
          motion={motion}
          frameloop={frameloopFor(motion, hidden, reduced)}
          still={reduced}
          dark={dark}
          onLost={() => setBroken(true)}
        />
      </Suspense>
    </Fallback>
  );
}

/** Tests only: forget the cached WebGL check. */
export function resetWebglCacheForTests(): void {
  webglCache = null;
}
