import { invoke } from "@tauri-apps/api/core";
import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";
import type { SnapshotListener, Transport } from "./index";

export class TauriTransport implements Transport {
  readonly isMock = false;

  ping(): Promise<PingResponse> {
    return invoke<PingResponse>("ping");
  }

  getSnapshot(): Promise<Snapshot> {
    return invoke<Snapshot>("get_snapshot");
  }

  // The backend does not publish `state.snapshot` events yet; that needs the
  // Envelope contract (spec §5.3), which goes in its own contract PR.
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  onSnapshot(_listener: SnapshotListener): () => void {
    return () => {};
  }

  // `cancel` and `set_mute` are agreed in docs/contracts.md and implemented in
  // Rust in M2; until then these reject and the UI shows the error.
  cancel(requestId: string): Promise<void> {
    return invoke("cancel", { requestId });
  }

  setMute(muted: boolean): Promise<void> {
    return invoke("set_mute", { muted });
  }
}
