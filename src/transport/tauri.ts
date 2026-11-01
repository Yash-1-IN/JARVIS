import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PROTOCOL_VERSION } from "../protocol";
import type { Envelope } from "../types/generated/Envelope";
import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";
import type { SnapshotListener, Transport, TurnListener } from "./index";

export const SNAPSHOT_EVENT = "state_snapshot";

export class TauriTransport implements Transport {
  readonly isMock = false;

  ping(): Promise<PingResponse> {
    return invoke<PingResponse>("ping");
  }

  getSnapshot(): Promise<Snapshot> {
    return invoke<Snapshot>("get_snapshot");
  }

  /**
   * The backend broadcasts `Envelope<Snapshot>` with kind `state.snapshot` on
   * the Tauri event `state_snapshot` (event names cannot contain '.').
   * Envelopes from another protocol version are dropped; the startup `ping`
   * check already blocks the UI on a mismatch.
   */
  onSnapshot(listener: SnapshotListener): () => void {
    let stopped = false;
    const unlisten = listen<Envelope<Snapshot>>(SNAPSHOT_EVENT, (e) => {
      const env = e.payload;
      if (env.v !== PROTOCOL_VERSION || env.kind !== "state.snapshot") return;
      listener(env.payload);
    });
    return () => {
      if (stopped) return;
      stopped = true;
      void unlisten.then((fn) => fn());
    };
  }

  // `submit_text` is implemented in Rust later in M2; until then it rejects
  // and the UI shows the error.
  cancel(requestId: string): Promise<void> {
    return invoke("cancel", { requestId });
  }

  submitText(text: string, onEvent: TurnListener): Promise<string> {
    const channel = new Channel<TurnStreamEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("submit_text", { text, onEvent: channel });
  }

  setMute(muted: boolean): Promise<void> {
    return invoke("set_mute", { muted });
  }
}
