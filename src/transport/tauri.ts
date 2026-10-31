import { invoke } from "@tauri-apps/api/core";
import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";
import type { Transport } from "./index";

export class TauriTransport implements Transport {
  ping(): Promise<PingResponse> {
    return invoke<PingResponse>("ping");
  }
  getSnapshot(): Promise<Snapshot> {
    return invoke<Snapshot>("get_snapshot");
  }
}
