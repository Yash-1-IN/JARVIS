import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useRef } from "react";
import {
  AdditiveBlending,
  type Blending,
  BufferAttribute,
  BufferGeometry,
  EdgesGeometry,
  Float32BufferAttribute,
  type Group,
  IcosahedronGeometry,
  type Mesh,
  NormalBlending,
  type Points,
} from "three";
import type { AssistantPhase } from "../../types/generated/AssistantPhase";
import type { MicState } from "../../types/generated/MicState";
import type { OrbMotion } from "../Orb2D";
import { ease, energyFor, FLARE_SECONDS, flareAt, sparkle } from "./motion";
import { contextLostHandler } from "./renderer";

/**
 * Amber and gold, in the manner of a sci-fi interface. Error is the only red, and
 * also changes shape; shape and motion carry every state, never colour alone. On a
 * dark background light adds up (a glow); on a light one it would vanish, so the
 * light theme draws solid, deeper colours instead.
 */
const PALETTE = {
  dark: {
    main: "#ffb13b",
    hot: "#ffe2a0",
    error: "#ff5a5a",
    muted: "#8a8f98",
  },
  light: {
    main: "#c2570c",
    hot: "#e07b1a",
    error: "#b91c1c",
    muted: "#59636e",
  },
};

const SPARKS = 220;
const TILTS: [number, number, number][] = [
  [Math.PI / 2, 0, 0],
  [Math.PI / 2.6, 0.7, 0],
  [Math.PI / 1.7, -0.9, 0.4],
];

/** Glowing lattice lines: the edges of a subdivided icosahedron. */
function lattice(radius: number, detail: number): BufferGeometry {
  return new EdgesGeometry(new IcosahedronGeometry(radius, detail), 1);
}

/** Sparks: a unit direction each, spread evenly over a sphere (Fibonacci lattice). */
function sparkDirections(count: number): Float32Array {
  const out = new Float32Array(count * 3);
  const golden = Math.PI * (3 - Math.sqrt(5));
  for (let i = 0; i < count; i++) {
    const y = 1 - (i / (count - 1)) * 2;
    const r = Math.sqrt(1 - y * y);
    out[i * 3] = Math.cos(golden * i) * r;
    out[i * 3 + 1] = y;
    out[i * 3 + 2] = Math.sin(golden * i) * r;
  }
  return out;
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
  const outer = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const core = useRef<Mesh>(null);
  const rings = useRef<(Mesh | null)[]>([]);
  const sparks = useRef<Points>(null);
  // The energy eased toward its target, and when the state last changed (for the flare).
  const smooth = useRef(0);
  const changedAt = useRef(Number.NEGATIVE_INFINITY);
  const { clock, invalidate } = useThree();
  const first = useRef(true);
  // A change of state (not the first one on screen) flares once and renders a few frames to let
  // it play out, even when idle, where the scene otherwise renders on demand and then stops.
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (still) return;
    changedAt.current = clock.getElapsedTime();
    invalidate();
  }, [phase, mic, still, clock, invalidate]);

  const error = phase === "error";
  const muted = mic === "muted";
  const tones = dark ? PALETTE.dark : PALETTE.light;
  const colour = error ? tones.error : muted ? tones.muted : tones.main;
  const hot = error ? tones.error : muted ? tones.muted : tones.hot;
  const blend: Blending = dark ? AdditiveBlending : NormalBlending;
  const dim = muted ? 0.4 : 1;
  // Light additive layers are faint; solid ones need to be stronger to read.
  const k = dark ? 0.55 : 0.85;

  const outerGeo = useMemo(() => lattice(1.55, 2), []);
  const innerGeo = useMemo(() => lattice(1.0, 1), []);
  const directions = useMemo(() => sparkDirections(SPARKS), []);
  const sparkGeo = useMemo(() => {
    const g = new BufferGeometry();
    g.setAttribute(
      "position",
      new Float32BufferAttribute(directions.slice(), 3),
    );
    return g;
  }, [directions]);

  // Runs only on rendered frames: every frame while animating, one per change
  // when idle (frameloop "demand"), none while hidden.
  useFrame((state, delta) => {
    const t = still ? 0 : state.clock.getElapsedTime();
    const target = still ? 0 : energyFor(motion, t);
    const flare = still ? 0 : flareAt(t - changedAt.current);
    smooth.current = still ? 0 : ease(smooth.current, target, delta);
    const e = Math.min(1, smooth.current + 0.65 * flare);
    if (!group.current || !core.current) return;
    // Idle renders on demand, so keep asking for frames only until the change has settled.
    if (
      !still &&
      (t - changedAt.current < FLARE_SECONDS ||
        Math.abs(smooth.current - target) > 0.005)
    ) {
      state.invalidate();
    }

    // The whole hologram shivers and swells with the energy: the jolt.
    const jolt =
      1 + 0.07 * e + (motion === "wave" ? 0.05 * e * Math.sin(t * 40) : 0);
    group.current.scale.setScalar(jolt);
    group.current.rotation.x = 0.35 + 0.04 * e * Math.sin(t * 17);

    // Two lattices turning against each other; thinking spins them harder.
    const spin = motion === "spin" ? 2.2 : 1;
    if (outer.current)
      outer.current.rotation.set(t * 0.12 * spin, t * 0.2 * spin, 0);
    if (inner.current)
      inner.current.rotation.set(-t * 0.25 * spin, -t * 0.34 * spin, t * 0.1);

    // The core flares with the energy.
    core.current.scale.setScalar(0.85 + 0.9 * e);
    (core.current.material as { opacity: number }).opacity =
      dim * Math.min(1, 0.45 + 0.9 * e);

    // Nested rings at different tilts; their radius jolts with the energy.
    rings.current.forEach((m, i) => {
      if (!m) return;
      const dir = i % 2 ? -1 : 1;
      m.rotation.z = dir * t * (0.25 + i * 0.35) * spin;
      m.scale.setScalar(
        1 + 0.12 * e * (1 + i * 0.4) * (0.6 + 0.4 * Math.sin(t * 9 + i)),
      );
    });

    // Sparks fly outward in bursts, each flickering on its own beat.
    const pts = sparks.current;
    if (pts) {
      const pos = (pts.geometry.getAttribute("position") as BufferAttribute)
        .array as Float32Array;
      for (let i = 0; i < SPARKS; i++) {
        const burst = e * e * 1.4 * sparkle(i, t);
        const r = 1.7 + 0.12 * Math.sin(t * 0.9 + i) + burst;
        pos[i * 3] = directions[i * 3] * r;
        pos[i * 3 + 1] = directions[i * 3 + 1] * r;
        pos[i * 3 + 2] = directions[i * 3 + 2] * r;
      }
      pts.geometry.getAttribute("position").needsUpdate = true;
      pts.rotation.y = t * 0.06;
    }
  });

  return (
    <group ref={group}>
      <group ref={outer}>
        <lineSegments geometry={outerGeo}>
          <lineBasicMaterial
            color={colour}
            transparent
            opacity={dim * k * 0.7}
            blending={blend}
            depthWrite={false}
          />
        </lineSegments>
      </group>
      <group ref={inner}>
        {/* A different solid for an error, so it never rests on colour alone. */}
        <lineSegments
          geometry={useMemo(
            () => (error ? lattice(0.95, 0) : innerGeo),
            [error, innerGeo],
          )}
        >
          <lineBasicMaterial
            color={hot}
            transparent
            opacity={dim * k}
            blending={blend}
            depthWrite={false}
          />
        </lineSegments>
      </group>
      <mesh ref={core}>
        <sphereGeometry args={[0.3, 20, 20]} />
        <meshBasicMaterial
          color={hot}
          transparent
          opacity={0.6}
          blending={blend}
          depthWrite={false}
        />
      </mesh>
      {TILTS.map((tilt, i) => (
        <mesh
          key={i}
          ref={(m) => {
            rings.current[i] = m;
          }}
          rotation={tilt}
        >
          <torusGeometry
            args={[
              1.25 + i * 0.28,
              0.014,
              6,
              120,
              muted ? Math.PI : Math.PI * (1.8 - i * 0.2),
            ]}
          />
          <meshBasicMaterial
            color={i === 0 ? hot : colour}
            transparent
            opacity={dim * (dark ? 0.7 : 0.9)}
            blending={blend}
            depthWrite={false}
          />
        </mesh>
      ))}
      <points ref={sparks} geometry={sparkGeo}>
        <pointsMaterial
          color={hot}
          size={0.035}
          transparent
          opacity={dim * (dark ? 0.8 : 0.9)}
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
        camera={{ position: [0, 0, 5.6], fov: 45 }}
        gl={{ alpha: true, antialias: true, powerPreference: "low-power" }}
        onCreated={({ gl }) => {
          gl.domElement.addEventListener(
            "webglcontextlost",
            contextLostHandler(gl.domElement, onLost),
          );
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
