import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";

export type SnapshotListener = (snapshot: Snapshot) => void;

/** One interface, two implementations: TauriTransport and MockTransport. */
export interface Transport {
  /** True only for MockTransport; drives the persistent MOCK badge. */
  readonly isMock: boolean;
  ping(): Promise<PingResponse>;
  getSnapshot(): Promise<Snapshot>;
  /** Subscribe to snapshot updates. Returns an unsubscribe function. */
  onSnapshot(listener: SnapshotListener): () => void;
  /** Cancels the whole request tree for `requestId` (contract: `cancel`). */
  cancel(requestId: string): Promise<void>;
  /** Contract: `set_mute`. */
  setMute(muted: boolean): Promise<void>;
}
