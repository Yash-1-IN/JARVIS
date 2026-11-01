import { Canvas, useFrame } from "@react-three/fiber";
import { useMemo, useRef } from "react";
import {
  AdditiveBlending,
  type Blending,
  BufferGeometry,
  NormalBlending,
  Float32BufferAttribute,
  type Group,
  type Mesh,
  type Points,
} from "three";
import type { AssistantPhase } from "../../types/generated/AssistantPhase";
import type { MicState } from "../../types/generated/MicState";
import type { OrbMotion } from "../Orb2D";

/**
 * Hologram colours; error is the only warm one, and shape and motion carry the
 * state too. On a dark background light adds up (a glow); on a light one it
 * would vanish, so the light theme draws solid, deeper colours instead.
 */
const PALETTE = {
  dark: { main: "#5ec8ff", error: "#ff5a5a", muted: "#7f8b99" },
  light: { main: "#2563eb", error: "#b91c1c", muted: "#59636e" },
};

/** Points spread evenly over a sphere (Fibonacci lattice): the particle shell. */
function shell(count: number, radius: number): BufferGeometry {
  const pos: number[] = [];
  const golden = Math.PI * (3 - Math.sqrt(5));
  for (let i = 0; i < count; i++) {
    const y = 1 - (i / (count - 1)) * 2;
    const r = Math.sqrt(1 - y * y);
    const t = golden * i;
    pos.push(Math.cos(t) * r * radius, y * radius, Math.sin(t) * r * radius);
  }
  const g = new BufferGeometry();
  g.setAttribute("position", new Float32BufferAttribute(pos, 3));
  return g;
}

interface SceneProps {
  phase: AssistantPhase;
  mic: MicState;
  motion: OrbMotion;
  /** With reduced motion every frame is the resting pose. */
  still: boolean;
  dark: boolean;
}

function Scene({ phase, mic, motion, still, dark }: SceneProps) {
  const group = useRef<Group>(null);
  const core = useRef<Mesh>(null);
  const ring = useRef<Mesh>(null);
  const halo = useRef<Mesh>(null);
  const dots = useRef<Points>(null);
  const particles = useMemo(() => shell(360, 1.55), []);

  const error = phase === "error";
  const muted = mic === "muted";
  const tones = dark ? PALETTE.dark : PALETTE.light;
  const colour = error ? tones.error : muted ? tones.muted : tones.main;
  const blend: Blending = dark ? AdditiveBlending : NormalBlending;
  const opacity = muted ? 0.35 : dark ? 0.6 : 0.85;

  // Runs only on rendered frames: every frame while animating, one per change
  // when idle (frameloop "demand"), none while hidden.
  useFrame((state) => {
    const t = still ? 0 : state.clock.getElapsedTime();
    const g = group.current;
    if (!g || !core.current || !ring.current || !halo.current) return;
    const pulse =
      motion === "pulse"
        ? 1 + 0.12 * Math.sin(t * 5.2)
        : motion === "breathe"
          ? 1 + 0.06 * Math.sin(t * 2.1)
          : 1;
    core.current.scale.setScalar(pulse);
    core.current.rotation.set(t * 0.25, t * 0.4, 0);
    ring.current.rotation.z = motion === "spin" ? t * 2.4 : t * 0.2;
    const wave = motion === "wave" ? (t * 1.1) % 1 : 0;
    halo.current.scale.setScalar(1 + wave * 0.9);
    (halo.current.material as { opacity: number }).opacity =
      motion === "wave" ? 0.5 * (1 - wave) : 0;
    if (dots.current) dots.current.rotation.y = t * 0.08;
    g.rotation.x = 0.35;
  });

  return (
    <group ref={group}>
      <mesh ref={core}>
        {/* A different solid for an error, so it never rests on colour alone. */}
        {error ? (
          <octahedronGeometry args={[0.95, 0]} />
        ) : (
          <icosahedronGeometry args={[0.95, 1]} />
        )}
        <meshBasicMaterial
          color={colour}
          wireframe
          transparent
          opacity={opacity}
          blending={blend}
          depthWrite={false}
        />
      </mesh>
      <mesh>
        <sphereGeometry args={[0.55, 24, 24]} />
        <meshBasicMaterial
          color={colour}
          transparent
          opacity={muted ? 0.08 : 0.22}
          blending={blend}
          depthWrite={false}
        />
      </mesh>
      <mesh ref={ring} rotation={[Math.PI / 2, 0, 0]}>
        <torusGeometry
          args={[1.4, 0.018, 8, 120, muted ? Math.PI : Math.PI * 1.7]}
        />
        <meshBasicMaterial
          color={colour}
          transparent
          opacity={opacity}
          blending={blend}
          depthWrite={false}
        />
      </mesh>
      <mesh ref={halo} rotation={[Math.PI / 2, 0, 0]}>
        <torusGeometry args={[1.4, 0.01, 6, 120]} />
        <meshBasicMaterial
          color={colour}
          transparent
          opacity={0}
          blending={blend}
          depthWrite={false}
        />
      </mesh>
      <points ref={dots} geometry={particles}>
        <pointsMaterial
          color={colour}
          size={0.025}
          transparent
          opacity={muted ? 0.2 : 0.55}
          blending={blend}
          depthWrite={false}
        />
      </points>
    </group>
  );
}

export interface Hologram3DProps {
  phase: AssistantPhase;
  mic: MicState;
  motion: OrbMotion;
  frameloop: "always" | "demand" | "never";
  still: boolean;
  /** WebGL context lost: the caller switches to the 2D orb. */
  onLost: () => void;
  /** The window's colour scheme. */
  dark: boolean;
}

/** The 3D hologram (ADR-015): Three.js through R3F, rendered on demand. */
export default function Hologram3D({
  phase,
  mic,
  motion,
  frameloop,
  still,
  onLost,
  dark,
}: Hologram3DProps) {
  return (
    <div
      className="hologram-3d"
      data-testid="hologram-3d"
      data-frameloop={frameloop}
      aria-hidden="true"
    >
      <Canvas
        frameloop={frameloop}
        dpr={[1, 2]}
        camera={{ position: [0, 0, 5.2], fov: 45 }}
        gl={{ alpha: true, antialias: true, powerPreference: "low-power" }}
        onCreated={({ gl }) => {
          gl.domElement.addEventListener("webglcontextlost", (e) => {
            e.preventDefault();
            onLost();
          });
        }}
      >
        <Scene
          phase={phase}
          mic={mic}
          motion={motion}
          still={still}
          dark={dark}
        />
      </Canvas>
    </div>
  );
}
