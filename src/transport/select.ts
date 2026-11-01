import type { Transport } from "./index";
import { TauriTransport } from "./tauri";

/**
 * MockTransport is only reachable in dev builds with VITE_TRANSPORT=mock.
 * In release builds `import.meta.env.DEV` is the literal `false`, so the
 * dynamic import is dead code and the mock is not bundled.
 */
export async function createTransport(): Promise<Transport> {
  if (import.meta.env.DEV && import.meta.env.VITE_TRANSPORT === "mock") {
    const { MockTransport } = await import("./mock/MockTransport");
    return new MockTransport();
  }
  return new TauriTransport();
}
