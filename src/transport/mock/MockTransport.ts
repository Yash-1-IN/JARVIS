import { PROTOCOL_VERSION } from "../../protocol";
import type { PingResponse } from "../../types/generated/PingResponse";
import type { Snapshot } from "../../types/generated/Snapshot";
import type { SnapshotListener, Transport } from "../index";
import { MOCK_SCRIPTS, type MockScript, type SnapshotPatch } from "./scripts";

const APPROVAL_TTL_MS = 60_000;

function initialSnapshot(): Snapshot {
  return {
    seq: 0,
    mic: "off",
    phase: "idle",
    activeRequestId: null,
    network: "unknown",
    cloudInUse: false,
    pendingApprovals: [],
    lastError: null,
  };
}

/**
 * Dev-only stand-in for the Rust backend (spec §5.5). It replays scripted
 * snapshots; it never decides state transitions on its own.
 */
export class MockTransport implements Transport {
  readonly isMock = true;
  readonly scripts: readonly MockScript[];
  private snapshot = initialSnapshot();
  private listeners = new Set<SnapshotListener>();
  private timers: ReturnType<typeof setTimeout>[] = [];

  constructor(scripts: readonly MockScript[] = MOCK_SCRIPTS) {
    this.scripts = scripts;
  }

  async ping(): Promise<PingResponse> {
    return {
      pong: true,
      protocolVersion: PROTOCOL_VERSION,
      appVersion: "mock",
    };
  }

  async getSnapshot(): Promise<Snapshot> {
    return this.snapshot;
  }

  onSnapshot(listener: SnapshotListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Cancels any running script and plays `name` from its first step. */
  runScript(name: string): void {
    const script = this.scripts.find((s) => s.name === name);
    if (!script) throw new Error(`Unknown mock script: ${name}`);
    this.stop();
    let at = 0;
    for (const step of script.steps) {
      at += step.delayMs;
      this.timers.push(setTimeout(() => this.apply(step.patch), at));
    }
  }

  async cancel(requestId: string): Promise<void> {
    if (this.snapshot.activeRequestId !== requestId) return;
    this.stop();
    this.apply({
      phase: "idle",
      activeRequestId: null,
      mic: this.snapshot.mic === "capturing" ? "standby" : this.snapshot.mic,
      cloudInUse: false,
      pendingApprovals: [],
    });
  }

  async setMute(muted: boolean): Promise<void> {
    if (this.snapshot.mic === "unavailable") return;
    this.apply({ mic: muted ? "muted" : "standby" });
  }

  stop(): void {
    this.timers.forEach(clearTimeout);
    this.timers = [];
  }

  private apply(patch: SnapshotPatch): void {
    const now = Date.now();
    const pendingApprovals = patch.pendingApprovals?.map((a) => ({
      ...a,
      expiresAt: a.expiresAt === 0 ? now + APPROVAL_TTL_MS : a.expiresAt,
    }));
    this.snapshot = {
      ...this.snapshot,
      ...patch,
      ...(pendingApprovals && { pendingApprovals }),
      seq: this.snapshot.seq + 1,
    };
    for (const l of this.listeners) l(this.snapshot);
  }
}
