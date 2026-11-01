import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";

export type SnapshotListener = (snapshot: Snapshot) => void;
export type TurnListener = (event: TurnStreamEvent) => void;

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
  /**
   * Starts a text turn (contract: `submit_text`). Stream events arrive on
   * `onEvent`, possibly before the returned requestId resolves; the stream
   * always ends with exactly one `done`.
   */
  submitText(text: string, onEvent: TurnListener): Promise<string>;
  /** Contract: `set_mute`. */
  setMute(muted: boolean): Promise<void>;
}
