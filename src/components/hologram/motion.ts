import type { OrbMotion } from "../Orb2D";

/** A cheap, repeatable pseudo-random number in [0, 1) from an integer step. */
function hash(n: number): number {
  const x = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * How energetic the hologram is at time `t` (seconds), 0 to 1. The 3D scene turns
 * this into core flare, ring jolts and sparks.
 *
 * - `none`: 0, completely still (idle renders no frames at all).
 * - `pulse` (listening): a calm, even beat.
 * - `spin` (thinking): steady, mid-level energy.
 * - `breathe` (waiting for approval): a slow swell.
 * - `wave` (speaking): an irregular, syllable-like jolt, like a voice: a phrase
 *   envelope, a fast beat, and a hard random kick several times a second.
 *
 * There is no audio yet (voice is M3). When text-to-speech exists the backend can
 * send the real loudness instead; the scene only needs a number from 0 to 1.
 */
export function energyFor(motion: OrbMotion, t: number): number {
  switch (motion) {
    case "none":
      return 0;
    case "pulse":
      return 0.25 + 0.2 * Math.sin(t * 4.4);
    case "spin":
      return 0.5;
    case "breathe":
      return 0.25 + 0.15 * Math.sin(t * 2.1);
    case "wave": {
      const phrase = 0.55 + 0.45 * Math.sin(t * 1.7 + Math.sin(t * 0.6) * 2);
      const syllable = Math.abs(Math.sin(t * 6.3 + Math.sin(t * 2.9) * 1.6));
      // A new random kick about every 110 ms that decays quickly: the "jolt".
      const step = t * 9;
      const kick = hash(Math.floor(step)) * Math.exp(-(step % 1) * 3.2);
      return clamp01(0.15 + 0.55 * phrase * syllable + 0.6 * kick * phrase);
    }
  }
}

/** Smooth 0..1 flicker for one spark, so each flickers on its own beat. */
export function sparkle(seed: number, t: number): number {
  return 0.5 + 0.5 * Math.sin(t * (3 + hash(seed) * 6) + hash(seed + 7) * 6.28);
}

/**
 * Moves `current` toward `target` by the time that has passed (`dt` seconds), so a change of state
 * eases in instead of jumping. `rate` is how fast it closes the gap: about 1 / rate seconds to
 * cover most of it. Frame-rate independent.
 */
export function ease(
  current: number,
  target: number,
  dt: number,
  rate = 7,
): number {
  return current + (target - current) * (1 - Math.exp(-rate * Math.max(0, dt)));
}

/** The one-off flare decays like this per second. */
const FLARE_DECAY = 4.5;
/** After this many seconds the flare is below 1% and the scene can stop rendering. */
export const FLARE_SECONDS = Math.log(100) / FLARE_DECAY;

/**
 * The flare when the state changes: 1 at the moment of change, then fading away. The scene adds
 * it to the energy, so the core flares, the rings jolt and the sparks burst, once.
 * `since` is the time in seconds since the change; before it (or never) there is no flare.
 */
export function flareAt(since: number): number {
  return since < 0 ? 0 : Math.exp(-FLARE_DECAY * since);
}
