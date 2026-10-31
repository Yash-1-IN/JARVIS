import type { PingResponse } from "../types/generated/PingResponse";
import type { Snapshot } from "../types/generated/Snapshot";

/** One interface, two implementations (TauriTransport now, MockTransport in M1). */
export interface Transport {
  ping(): Promise<PingResponse>;
  getSnapshot(): Promise<Snapshot>;
}
