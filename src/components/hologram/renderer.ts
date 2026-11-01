import { create } from "zustand";
import type { OrbMotion } from "../Orb2D";

/** The user's choice in Settings → Display. */
export type HologramPref = "auto" | "3d" | "2d";
export type Renderer = "3d" | "2d";

/**
 * Which hologram to draw (ADR-015, spec §13 M1): 2D when WebGL is missing or
 * the user chose it; on Linux, 2D unless the user asks for 3D, because
 * WebKitGTK can fall back to a slow WebGL path without saying so (D-08).
 */
export function chooseRenderer(
  pref: HologramPref,
  env: { webgl: boolean; linux: boolean },
): Renderer {
  if (!env.webgl || pref === "2d") return "2d";
  if (pref === "3d") return "3d";
  return env.linux ? "2d" : "3d";
}

/**
 * The 3D render loop for a state (spec §10): continuous only while something
 * moves; on demand (one frame per change, then nothing) when idle or with
 * reduced motion; never while the window is hidden.
 */
export function frameloopFor(
  motion: OrbMotion,
  hidden: boolean,
  reducedMotion: boolean,
): "always" | "demand" | "never" {
  if (hidden) return "never";
  if (motion === "none" || reducedMotion) return "demand";
  return "always";
}

export function hasWebGL(): boolean {
  try {
    const c = document.createElement("canvas");
    return Boolean(c.getContext("webgl2") ?? c.getContext("webgl"));
  } catch {
    return false;
  }
}

export function isLinux(): boolean {
  const ua = navigator.userAgent;
  return /Linux/i.test(ua) && !/Android/i.test(ua);
}

const KEY = "jarvis.hologram";

function readPref(): HologramPref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "3d" || v === "2d" ? v : "auto";
  } catch {
    return "auto";
  }
}

/**
 * A display preference only, kept in the webview's local storage: it is not
 * assistant state and holds nothing sensitive, so it stays out of the
 * backend's settings contract.
 */
export const useHologramPref = create<{
  pref: HologramPref;
  /** Goes up on every choice, so a 3D failure is only remembered until the next one. */
  choice: number;
  setPref: (p: HologramPref) => void;
}>((set) => ({
  pref: readPref(),
  choice: 0,
  setPref: (pref) => {
    try {
      localStorage.setItem(KEY, pref);
    } catch {
      // Storage can be unavailable; the choice still applies until restart.
    }
    set((s) => ({ pref, choice: s.choice + 1 }));
  },
}));

/**
 * The `webglcontextlost` handler: a real loss while the canvas is on screen
 * means 3D failed. R3F also forces a context loss when the 3D view unmounts
 * (switching to 2D); by then the canvas is detached, so that one is ignored.
 */
export function contextLostHandler(
  canvas: HTMLCanvasElement,
  onLost: () => void,
): (e: Event) => void {
  return (e) => {
    if (!canvas.isConnected) return;
    e.preventDefault();
    onLost();
  };
}
