import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

import { PROTOCOL_VERSION } from "../protocol";
import { TauriTransport } from "./tauri";

describe("TauriTransport", () => {
  beforeEach(() => invoke.mockReset());

  it("ping calls the ping command and returns its payload", async () => {
    invoke.mockResolvedValue({
      pong: true,
      protocolVersion: PROTOCOL_VERSION,
      appVersion: "0.1.0",
    });
    const res = await new TauriTransport().ping();
    expect(invoke).toHaveBeenCalledWith("ping");
    expect(res.pong).toBe(true);
    expect(res.protocolVersion).toBe(PROTOCOL_VERSION);
  });

  it("getSnapshot calls get_snapshot", async () => {
    invoke.mockResolvedValue({ seq: 0, mic: "off", phase: "idle" });
    const snap = await new TauriTransport().getSnapshot();
    expect(invoke).toHaveBeenCalledWith("get_snapshot");
    expect(snap.phase).toBe("idle");
  });
});
